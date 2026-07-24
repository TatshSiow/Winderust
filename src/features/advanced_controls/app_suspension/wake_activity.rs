use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NetworkActivityCounters {
    pub(super) bytes_in: u64,
    pub(super) bytes_out: u64,
}

impl NetworkActivityCounters {
    pub(super) fn exceeds_threshold_since(
        self,
        previous: Self,
        thresholds: NetworkActivityThresholds,
    ) -> bool {
        let bytes_in = self.bytes_in.saturating_sub(previous.bytes_in);
        let bytes_out = self.bytes_out.saturating_sub(previous.bytes_out);
        if thresholds.bytes_in == 0 && thresholds.bytes_out == 0 {
            return bytes_in > 0 || bytes_out > 0;
        }
        (thresholds.bytes_in > 0 && bytes_in >= thresholds.bytes_in)
            || (thresholds.bytes_out > 0 && bytes_out >= thresholds.bytes_out)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NetworkActivityThresholds {
    pub(super) bytes_in: u64,
    pub(super) bytes_out: u64,
}

pub(super) fn network_wake_duration(settings: &AppSuspensionSettings) -> Option<Duration> {
    (settings.network_wake_enabled && settings.network_wake_duration_seconds > 0)
        .then(|| bounded_suspension_duration(settings.network_wake_duration_seconds))
}

pub(super) fn audio_wake_duration(settings: &AppSuspensionSettings) -> Option<Duration> {
    (settings.audio_wake_enabled && settings.audio_wake_duration_seconds > 0)
        .then(|| bounded_suspension_duration(settings.audio_wake_duration_seconds))
}

pub(super) fn audio_process_names_with_activity(
    target_processes: &BTreeMap<u32, String>,
) -> Result<BTreeSet<String>, String> {
    if target_processes.is_empty() {
        return Ok(BTreeSet::new());
    }

    let active_process_ids = active_audio_process_ids()?;
    Ok(target_processes
        .iter()
        .filter(|(process_id, _process_name)| active_process_ids.contains(process_id))
        .map(|(_process_id, process_name)| process_name_key(process_name))
        .collect())
}

pub(super) fn network_connection_snapshot(
    target_processes: &BTreeMap<u32, String>,
) -> Result<NetworkConnectionSnapshot, String> {
    let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
    let mut connections_by_pid: NetworkConnectionsByProcess = BTreeMap::new();

    add_tcp_connections(&mut connections_by_pid, &target_ids, AF_INET as u32)?;
    add_tcp_connections(&mut connections_by_pid, &target_ids, AF_INET6 as u32)?;
    add_udp_connections(&mut connections_by_pid, &target_ids, AF_INET as u32)?;
    add_udp_connections(&mut connections_by_pid, &target_ids, AF_INET6 as u32)?;

    let mut snapshot = target_processes
        .values()
        .map(|process_name| (process_name_key(process_name), BTreeMap::new()))
        .collect::<NetworkConnectionSnapshot>();
    for (process_id, connections) in connections_by_pid {
        let Some(process_name) = target_processes.get(&process_id) else {
            continue;
        };

        snapshot
            .entry(process_name_key(process_name))
            .or_insert_with(BTreeMap::new)
            .extend(connections);
    }

    Ok(snapshot)
}

pub(super) fn network_process_names_with_activity(
    previous: &NetworkConnectionSnapshot,
    current: &NetworkConnectionSnapshot,
    thresholds_by_process: &NetworkActivityThresholdsByProcess,
) -> BTreeSet<String> {
    current
        .iter()
        .filter(|(process_name, connections)| {
            let Some(thresholds) = thresholds_by_process.get(*process_name) else {
                return false;
            };
            previous
                .get(*process_name)
                .is_some_and(|previous_connections| {
                    connections.iter().any(|(connection, activity)| {
                        match previous_connections.get(connection) {
                            Some(Some(previous_activity)) => activity.is_some_and(|activity| {
                                activity.exceeds_threshold_since(*previous_activity, *thresholds)
                            }),
                            Some(None) => false,
                            None => false,
                        }
                    })
                })
        })
        .map(|(process_name, _connections)| process_name.clone())
        .collect()
}

pub(super) fn network_activity_thresholds(
    settings: &AppSuspensionSettings,
    target_processes: &BTreeMap<u32, String>,
) -> NetworkActivityThresholdsByProcess {
    target_processes
        .values()
        .filter_map(|process_name| {
            let (bytes_in, bytes_out) = settings.network_wake_thresholds_for(process_name)?;
            Some((
                process_name_key(process_name),
                NetworkActivityThresholds {
                    bytes_in,
                    bytes_out,
                },
            ))
        })
        .collect()
}

pub(super) fn eligible_network_wake_names(
    network_process_names: &BTreeSet<String>,
    network_target_process_names: &BTreeSet<String>,
) -> BTreeSet<String> {
    network_process_names
        .intersection(network_target_process_names)
        .cloned()
        .collect()
}

pub(super) fn manual_freeze_app_names(process_names: &[String]) -> BTreeSet<String> {
    process_names
        .iter()
        .map(|process_name| process_name_key(process_name))
        .filter(|process_name| !process_name.is_empty())
        .collect()
}

pub(super) fn add_tcp_connections(
    connections_by_pid: &mut NetworkConnectionsByProcess,
    target_ids: &BTreeSet<u32>,
    address_family: u32,
) -> Result<(), String> {
    let buffer = query_ip_helper_table(|table, size| {
        // SAFETY: query_ip_helper_table passes either a null sizing buffer or writable storage of
        // the byte count in size; address_family and table class are documented constants.
        unsafe {
            GetExtendedTcpTable(
                table,
                size,
                0,
                address_family,
                TCP_TABLE_OWNER_PID_CONNECTIONS,
                0,
            )
        }
    })?;

    if address_family == AF_INET as u32 {
        for row in table_rows::<MIB_TCPROW_OWNER_PID>(&buffer) {
            if target_ids.contains(&row.dwOwningPid) {
                let Some(connection) = tcp4_connection_key(&row) else {
                    continue;
                };
                let activity = tcp4_connection_activity(&row);

                connections_by_pid
                    .entry(row.dwOwningPid)
                    .or_default()
                    .insert(connection, activity);
            }
        }
    } else {
        for row in table_rows::<MIB_TCP6ROW_OWNER_PID>(&buffer) {
            if target_ids.contains(&row.dwOwningPid) {
                let Some(connection) = tcp6_connection_key(&row) else {
                    continue;
                };
                let activity = tcp6_connection_activity(&row);

                connections_by_pid
                    .entry(row.dwOwningPid)
                    .or_default()
                    .insert(connection, activity);
            }
        }
    }

    Ok(())
}

pub(super) fn tcp4_connection_key(row: &MIB_TCPROW_OWNER_PID) -> Option<String> {
    is_network_intent_tcp_state(row.dwState).then(|| {
        format!(
            "tcp4:{}:{}:{}:{}",
            row.dwLocalAddr, row.dwLocalPort, row.dwRemoteAddr, row.dwRemotePort
        )
    })
}

pub(super) fn tcp6_connection_key(row: &MIB_TCP6ROW_OWNER_PID) -> Option<String> {
    is_network_intent_tcp_state(row.dwState).then(|| {
        format!(
            "tcp6:{:?}:{}:{:?}:{}:{}:{}",
            row.ucLocalAddr,
            row.dwLocalScopeId,
            row.ucRemoteAddr,
            row.dwRemoteScopeId,
            row.dwLocalPort,
            row.dwRemotePort
        )
    })
}

pub(super) fn tcp4_connection_activity(
    row: &MIB_TCPROW_OWNER_PID,
) -> Option<NetworkActivityCounters> {
    let tcp_row = MIB_TCPROW_LH {
        Anonymous: MIB_TCPROW_LH_0 {
            dwState: row.dwState,
        },
        dwLocalAddr: row.dwLocalAddr,
        dwLocalPort: row.dwLocalPort,
        dwRemoteAddr: row.dwRemoteAddr,
        dwRemotePort: row.dwRemotePort,
    };
    enable_tcp4_data_stats(&tcp_row);
    tcp4_data_stats(&tcp_row)
}

pub(super) fn tcp6_connection_activity(
    row: &MIB_TCP6ROW_OWNER_PID,
) -> Option<NetworkActivityCounters> {
    let tcp_row = MIB_TCP6ROW {
        State: row.dwState as i32,
        LocalAddr: IN6_ADDR {
            u: IN6_ADDR_0 {
                Byte: row.ucLocalAddr,
            },
        },
        dwLocalScopeId: row.dwLocalScopeId,
        dwLocalPort: row.dwLocalPort,
        RemoteAddr: IN6_ADDR {
            u: IN6_ADDR_0 {
                Byte: row.ucRemoteAddr,
            },
        },
        dwRemoteScopeId: row.dwRemoteScopeId,
        dwRemotePort: row.dwRemotePort,
    };
    enable_tcp6_data_stats(&tcp_row);
    tcp6_data_stats(&tcp_row)
}

pub(super) fn enable_tcp4_data_stats(row: &MIB_TCPROW_LH) {
    let rw = TCP_ESTATS_DATA_RW_v0 {
        EnableCollection: true,
    };
    // SAFETY: row and rw are fully initialized for the synchronous call and the supplied size
    // matches TCP_ESTATS_DATA_RW_v0.
    unsafe {
        SetPerTcpConnectionEStats(
            row,
            TcpConnectionEstatsData,
            &rw as *const _ as *const u8,
            0,
            mem::size_of::<TCP_ESTATS_DATA_RW_v0>() as u32,
            0,
        );
    }
}

pub(super) fn enable_tcp6_data_stats(row: &MIB_TCP6ROW) {
    let rw = TCP_ESTATS_DATA_RW_v0 {
        EnableCollection: true,
    };
    // SAFETY: row and rw are fully initialized for the synchronous call and the supplied size
    // matches TCP_ESTATS_DATA_RW_v0.
    unsafe {
        SetPerTcp6ConnectionEStats(
            row,
            TcpConnectionEstatsData,
            &rw as *const _ as *const u8,
            0,
            mem::size_of::<TCP_ESTATS_DATA_RW_v0>() as u32,
            0,
        );
    }
}

pub(super) fn tcp4_data_stats(row: &MIB_TCPROW_LH) -> Option<NetworkActivityCounters> {
    let mut rod = TCP_ESTATS_DATA_ROD_v0::default();
    // SAFETY: row is fully initialized; unused buffers are null and rod is writable for exactly
    // the supplied output size.
    let status = unsafe {
        GetPerTcpConnectionEStats(
            row,
            TcpConnectionEstatsData,
            null_mut(),
            0,
            0,
            null_mut(),
            0,
            0,
            &mut rod as *mut _ as *mut u8,
            0,
            mem::size_of::<TCP_ESTATS_DATA_ROD_v0>() as u32,
        )
    };

    (status == NO_ERROR).then_some(NetworkActivityCounters {
        bytes_in: rod.DataBytesIn,
        bytes_out: rod.DataBytesOut,
    })
}

pub(super) fn tcp6_data_stats(row: &MIB_TCP6ROW) -> Option<NetworkActivityCounters> {
    let mut rod = TCP_ESTATS_DATA_ROD_v0::default();
    // SAFETY: row is fully initialized; unused buffers are null and rod is writable for exactly
    // the supplied output size.
    let status = unsafe {
        GetPerTcp6ConnectionEStats(
            row,
            TcpConnectionEstatsData,
            null_mut(),
            0,
            0,
            null_mut(),
            0,
            0,
            &mut rod as *mut _ as *mut u8,
            0,
            mem::size_of::<TCP_ESTATS_DATA_ROD_v0>() as u32,
        )
    };

    (status == NO_ERROR).then_some(NetworkActivityCounters {
        bytes_in: rod.DataBytesIn,
        bytes_out: rod.DataBytesOut,
    })
}

pub(super) fn is_network_intent_tcp_state(state: u32) -> bool {
    matches!(
        state,
        TCP_STATE_SYN_SENT | TCP_STATE_SYN_RECEIVED | TCP_STATE_ESTABLISHED
    )
}

pub(super) fn add_udp_connections(
    connections_by_pid: &mut NetworkConnectionsByProcess,
    target_ids: &BTreeSet<u32>,
    address_family: u32,
) -> Result<(), String> {
    let buffer = query_ip_helper_table(|table, size| {
        // SAFETY: query_ip_helper_table passes either a null sizing buffer or writable storage of
        // the byte count in size; address_family and table class are documented constants.
        unsafe { GetExtendedUdpTable(table, size, 0, address_family, UDP_TABLE_OWNER_PID, 0) }
    })?;

    if address_family == AF_INET as u32 {
        for row in table_rows::<MIB_UDPROW_OWNER_PID>(&buffer) {
            if target_ids.contains(&row.dwOwningPid) {
                connections_by_pid
                    .entry(row.dwOwningPid)
                    .or_default()
                    .insert(
                        format!("udp4:{}:{}", row.dwLocalAddr, row.dwLocalPort),
                        None,
                    );
            }
        }
    } else {
        for row in table_rows::<MIB_UDP6ROW_OWNER_PID>(&buffer) {
            if target_ids.contains(&row.dwOwningPid) {
                connections_by_pid
                    .entry(row.dwOwningPid)
                    .or_default()
                    .insert(
                        format!(
                            "udp6:{:?}:{}:{}",
                            row.ucLocalAddr, row.dwLocalScopeId, row.dwLocalPort
                        ),
                        None,
                    );
            }
        }
    }

    Ok(())
}

pub(super) fn query_ip_helper_table(
    mut query: impl FnMut(*mut c_void, *mut u32) -> u32,
) -> Result<Vec<u8>, String> {
    let mut size = 0;
    let first_status = query(null_mut(), &mut size);
    if first_status != ERROR_INSUFFICIENT_BUFFER && first_status != NO_ERROR {
        return Err(format!(
            "Network intent detection failed to size IP Helper table with error {first_status}."
        ));
    }

    if size == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0u8; size as usize];
    let mut status = query(buffer.as_mut_ptr() as *mut c_void, &mut size);
    while status == ERROR_INSUFFICIENT_BUFFER && size as usize > buffer.len() {
        buffer.resize(size as usize, 0);
        status = query(buffer.as_mut_ptr() as *mut c_void, &mut size);
    }
    if status != NO_ERROR {
        return Err(format!(
            "Network intent detection failed to read IP Helper table with error {status}."
        ));
    }

    Ok(buffer)
}

pub(super) fn table_rows<T: Copy>(buffer: &[u8]) -> Vec<T> {
    if buffer.len() < mem::size_of::<u32>() {
        return Vec::new();
    }

    // SAFETY: The length check above guarantees a complete u32 header; unaligned access matches
    // the byte-packed IP Helper table.
    let count = unsafe { ptr::read_unaligned(buffer.as_ptr() as *const u32) as usize };
    if count == 0 {
        return Vec::new();
    }

    let rows_offset = mem::size_of::<u32>();
    let row_size = mem::size_of::<T>();
    let Some(rows_len) = count.checked_mul(row_size) else {
        return Vec::new();
    };
    let Some(required_len) = rows_offset.checked_add(rows_len) else {
        return Vec::new();
    };
    if row_size == 0 || buffer.len() < required_len {
        return Vec::new();
    }

    // SAFETY: required_len was checked against buffer, so rows_offset is in bounds.
    let rows_ptr = unsafe { buffer.as_ptr().add(rows_offset) as *const T };
    (0..count)
        .map(|index| {
            // SAFETY: count and row_size were checked against buffer; each indexed row is fully
            // contained and read unaligned to match the packed table.
            unsafe { ptr::read_unaligned(rows_ptr.add(index)) }
        })
        .collect()
}

pub(super) fn should_skip_foreground_process(
    process_id: u32,
    process_name: &str,
    foreground_process_id: u32,
    foreground_process_name: Option<&str>,
) -> bool {
    process_id == foreground_process_id
        || foreground_process_name
            .is_some_and(|name| process_name_key(name) == process_name_key(process_name))
}
