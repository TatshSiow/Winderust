$ErrorActionPreference = 'Stop'

function Assert-NoUnexpectedWriter {
    param(
        [string]$Mechanism,
        [string]$ApiPattern,
        [string]$AllowedLocationPattern
    )

    $matches = @(& rg -n -S --glob '*.rs' $ApiPattern src)
    $searchExitCode = $LASTEXITCODE
    if ($searchExitCode -gt 1) {
        throw "Source search failed for $Mechanism with exit code $searchExitCode."
    }

    $unexpected = @($matches | Where-Object { $_ -notmatch $AllowedLocationPattern })
    if ($unexpected.Count -gt 0) {
        $unexpected | Write-Output
        throw "Unexpected raw writer or import for $Mechanism."
    }

    Write-Host "Architecture ownership gate passed: $Mechanism"
}

function Assert-NoSourceMatch {
    param(
        [string]$Boundary,
        [string]$Pattern,
        [string[]]$Paths
    )

    $matches = @(& rg -n -S --glob '*.rs' $Pattern @Paths)
    $searchExitCode = $LASTEXITCODE
    if ($searchExitCode -gt 1) {
        throw "Source search failed for $Boundary with exit code $searchExitCode."
    }
    if ($matches.Count -gt 0) {
        $matches | Write-Output
        throw "Architecture boundary violated: $Boundary."
    }

    Write-Host "Architecture ownership gate passed: $Boundary"
}

function Assert-SourceMatchCount {
    param(
        [string]$Boundary,
        [string]$Pattern,
        [string[]]$Paths,
        [int]$ExpectedCount
    )

    $matches = @(& rg -n -S --glob '*.rs' $Pattern @Paths)
    $searchExitCode = $LASTEXITCODE
    if ($searchExitCode -gt 1) {
        throw "Source search failed for $Boundary with exit code $searchExitCode."
    }
    if ($matches.Count -ne $ExpectedCount) {
        $matches | Write-Output
        throw "Architecture boundary violated: $Boundary expected $ExpectedCount match(es), found $($matches.Count)."
    }

    Write-Host "Architecture ownership gate passed: $Boundary"
}

Assert-NoUnexpectedWriter `
    -Mechanism 'process priority class' `
    -ApiPattern 'SetPriorityClass' `
    -AllowedLocationPattern '^src\\(?:backend\\crash_recovery|platform\\windows\\(?:priority_efficiency|self_power))\.rs:'

Assert-NoUnexpectedWriter `
    -Mechanism 'process power throttling and memory priority' `
    -ApiPattern 'SetProcessInformation' `
    -AllowedLocationPattern '^src\\(?:backend\\crash_recovery|platform\\windows\\(?:memory_priority|priority_efficiency|self_power))\.rs:'

Assert-SourceMatchCount `
    -Boundary 'single Process Priority production adapter call' `
    -Pattern 'SetPriorityClass\s*\(' `
    -Paths @('src/platform/windows/priority_efficiency.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'single process Power Throttling production adapter call' `
    -Pattern 'SetProcessInformation\s*\(' `
    -Paths @('src/platform/windows/priority_efficiency.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'single Winderust self-power priority adapter call' `
    -Pattern 'SetPriorityClass\s*\(' `
    -Paths @('src/platform/windows/self_power.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'single Winderust self-power throttling adapter call' `
    -Pattern 'SetProcessInformation\s*\(' `
    -Paths @('src/platform/windows/self_power.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'Winderust self-power controller owns lifecycle without raw Win32' `
    -Pattern 'GetCurrentProcess|GetPriorityClass|SetPriorityClass|GetProcessInformation|SetProcessInformation|PROCESS_POWER_THROTTLING_STATE|windows_sys|unsafe' `
    -Paths @('src/backend/self_power.rs')

Assert-NoSourceMatch `
    -Boundary 'Winderust self-power Windows adapter imports no policy layer' `
    -Pattern 'crate::(?:control|features|foreground|rules|ui)' `
    -Paths @('src/platform/windows/self_power.rs')

Assert-NoSourceMatch `
    -Boundary 'Priority and Efficiency controller owns compound transactions without raw Win32' `
    -Pattern 'GetPriorityClass|SetPriorityClass|GetProcessInformation|SetProcessInformation|PROCESS_POWER_THROTTLING_STATE|windows_sys|unsafe' `
    -Paths @('src/control/priority_efficiency.rs')

Assert-NoSourceMatch `
    -Boundary 'legacy Process Priority feature restore owner' `
    -Pattern 'fn apply_once\(|fn current_priority\(|struct AdjustedProcess|struct ProcessHandle|GetPriorityClass|SetPriorityClass|record_process_change' `
    -Paths @('src/features/priority_control/process_priority.rs')

Assert-NoSourceMatch `
    -Boundary 'legacy Background Efficiency mutation or restore owner' `
    -Pattern 'apply_efficiency_mode_once|current_efficiency_mode|struct ThrottledProcess|struct ProcessHandle|impl Drop|GetPriorityClass|SetPriorityClass|GetProcessInformation|SetProcessInformation|record_process_change|ProcessValue' `
    -Paths @('src/features/winderust_features/background_efficiency.rs')

Assert-NoSourceMatch `
    -Boundary 'legacy CPU Scheduler priority or Power Throttling owner' `
    -Pattern 'struct AdjustedProcess|struct BoostedProcess|previous_priority|applied_priority|restore_adjusted|restore_boosted|GetPriorityClass|SetPriorityClass|GetProcessInformation|SetProcessInformation|record_process_change|ProcessValue::PriorityClass|ProcessValue::power_throttling' `
    -Paths @(
        'src/features/winderust_features/cpu_scheduler.rs',
        'src/features/winderust_features/cpu_scheduler/process_control.rs'
    )

Assert-NoSourceMatch `
    -Boundary 'Process List does not own Process Priority or Efficiency Mode restoration' `
    -Pattern 'process_priority::apply_once|background_efficiency::apply_efficiency_mode_once|process_quick_action_restore|process_efficiency_mode_overrides' `
    -Paths @(
        'src/ui/app.rs',
        'src/ui/process_list.rs'
    )

Assert-SourceMatchCount `
    -Boundary 'single Memory Priority production adapter call' `
    -Pattern 'SetProcessInformation\s*\(' `
    -Paths @('src/platform/windows/memory_priority.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'Memory Priority controller owns transactions without raw Win32' `
    -Pattern 'GetProcessInformation|SetProcessInformation|MEMORY_PRIORITY_INFORMATION|windows_sys|unsafe' `
    -Paths @('src/control/memory_priority.rs')

Assert-NoSourceMatch `
    -Boundary 'legacy Memory Priority feature restore owner' `
    -Pattern 'fn apply_once\(|fn current_priority\(|struct AdjustedProcess|struct ProcessHandle|MEMORY_PRIORITY_INFORMATION|GetProcessInformation|SetProcessInformation|record_process_change' `
    -Paths @('src/features/priority_control/memory_priority.rs')

Assert-NoSourceMatch `
    -Boundary 'Process List does not own Memory Priority mutation or restoration' `
    -Pattern 'memory_priority::apply_once|memory_priority::current_priority|quick_apply_memory_priority|current_memory_priority\(' `
    -Paths @('src/ui/process_list.rs')

Assert-NoSourceMatch `
    -Boundary 'CPU Scheduler does not own a Memory Priority writer' `
    -Pattern 'MEMORY_PRIORITY_INFORMATION|ProcessMemoryPriorityClass|ProcessValue::MemoryPriority' `
    -Paths @(
        'src/features/winderust_features/cpu_scheduler.rs',
        'src/features/winderust_features/cpu_scheduler/process_control.rs'
    )

Assert-SourceMatchCount `
    -Boundary 'single Memory Trim production adapter call' `
    -Pattern 'SetProcessWorkingSetSize\s*\(' `
    -Paths @('src/platform/windows/memory_trim.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'Memory Trim controller owns command semantics without raw Win32' `
    -Pattern 'SetProcessWorkingSetSize|K32GetProcessMemoryInfo|GetProcessTimes|windows_sys|unsafe' `
    -Paths @('src/control/memory_trim.rs')

Assert-NoSourceMatch `
    -Boundary 'Memory Trim feature remains policy-only' `
    -Pattern 'SetProcessWorkingSetSize|K32GetProcessMemoryInfo|GetProcessTimes|PROCESS_SET_QUOTA|struct ProcessHandle' `
    -Paths @('src/features/winderust_features/memory_trim.rs')

Assert-NoSourceMatch `
    -Boundary 'Memory Trim remains irreversible and outside recovery ownership' `
    -Pattern 'RecoveryIntent|ProcessValue|record_process_change|baseline|managed|impl Drop' `
    -Paths @('src/control/memory_trim.rs')

Assert-NoSourceMatch `
    -Boundary 'manual Memory Trim is not retained as a delayed coalesced intent' `
    -Pattern 'memory_trim_now_requested' `
    -Paths @(
        'src/backend/automation.rs',
        'src/backend/automation/status.rs'
    )

Assert-NoUnexpectedWriter `
    -Mechanism 'thread priority' `
    -ApiPattern 'SetThreadPriority' `
    -AllowedLocationPattern '^src\\(?:backend\\crash_recovery|platform\\windows\\thread_priority)\.rs:'

Assert-SourceMatchCount `
    -Boundary 'single Thread Priority production adapter' `
    -Pattern 'SetThreadPriority\s*\(' `
    -Paths @('src/platform/windows/thread_priority.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'Thread Priority controller owns identity and transactions without raw Win32' `
    -Pattern 'CreateToolhelp32Snapshot|Thread32First|Thread32Next|OpenThread|GetProcessIdOfThread|GetThreadPriority|GetThreadTimes|SetThreadPriority|windows_sys|unsafe' `
    -Paths @('src/control/thread_priority.rs')

Assert-NoSourceMatch `
    -Boundary 'Thread Priority Windows adapter imports no policy layer' `
    -Pattern 'crate::(?:control|features|foreground|rules|ui)' `
    -Paths @('src/platform/windows/thread_priority.rs')

Assert-NoSourceMatch `
    -Boundary 'legacy Thread Priority feature restore owner' `
    -Pattern 'fn apply_once\(|fn current_priority\(|struct AdjustedThread|struct ThreadHandle|SetThreadPriority' `
    -Paths @('src/features/priority_control/thread_priority.rs')

Assert-NoSourceMatch `
    -Boundary 'Process List does not own Thread Priority mutation or restoration' `
    -Pattern 'quick_apply_thread_priority|thread_priority::apply_once|thread_priority::current_priority' `
    -Paths @('src/ui/process_list.rs')

Assert-NoUnexpectedWriter `
    -Mechanism 'Dynamic Priority Boost' `
    -ApiPattern 'SetProcessPriorityBoost' `
    -AllowedLocationPattern '^src\\(?:backend\\crash_recovery|platform\\windows\\dynamic_priority_boost)\.rs:'

Assert-SourceMatchCount `
    -Boundary 'single Dynamic Priority Boost production adapter' `
    -Pattern 'SetProcessPriorityBoost\s*\(' `
    -Paths @('src/platform/windows/dynamic_priority_boost.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'Dynamic Priority Boost controller owns transactions without raw Win32' `
    -Pattern 'GetProcessPriorityBoost|SetProcessPriorityBoost|windows_sys|unsafe' `
    -Paths @('src/control/dynamic_priority_boost.rs')

Assert-NoSourceMatch `
    -Boundary 'legacy Dynamic Priority Boost feature restore owner' `
    -Pattern 'fn apply_once\(|fn current_boost_disabled\(|struct AdjustedProcess' `
    -Paths @('src/features/priority_control/dynamic_priority_boost.rs')

Assert-NoSourceMatch `
    -Boundary 'legacy Dynamic Priority Boost CPU Scheduler capability' `
    -Pattern 'previous_dynamic_priority_boost_disabled|applied_dynamic_priority_boost_disabled|disable_dynamic_priority_boost|GetProcessPriorityBoost|SetProcessPriorityBoost' `
    -Paths @(
        'src/features/winderust_features/cpu_scheduler.rs',
        'src/features/winderust_features/cpu_scheduler/process_control.rs'
    )

Assert-NoUnexpectedWriter `
    -Mechanism 'I/O priority' `
    -ApiPattern 'NtSetInformationProcess' `
    -AllowedLocationPattern '^src\\(?:backend\\crash_recovery|platform\\windows\\io_priority)\.rs:'

Assert-SourceMatchCount `
    -Boundary 'single I/O Priority production adapter declaration and call' `
    -Pattern 'NtSetInformationProcess\s*\(' `
    -Paths @('src/platform/windows/io_priority.rs') `
    -ExpectedCount 2

Assert-NoSourceMatch `
    -Boundary 'I/O Priority controller owns transactions without raw NT calls' `
    -Pattern 'Nt(?:Query|Set)InformationProcess|windows_sys|unsafe extern|unsafe \{' `
    -Paths @('src/control/io_priority.rs')

Assert-NoSourceMatch `
    -Boundary 'legacy I/O Priority feature restore owner' `
    -Pattern 'fn apply_once\(|fn current_priority\(|struct AdjustedProcess|struct ProcessHandle|NtSetInformationProcess|record_process_change' `
    -Paths @('src/features/priority_control/io_priority.rs')

Assert-NoSourceMatch `
    -Boundary 'Process List does not own I/O Priority mutation or restoration' `
    -Pattern 'io_priority::apply_once|io_priority::current_priority|current_io_priority\(' `
    -Paths @('src/ui/process_list.rs')

Assert-NoSourceMatch `
    -Boundary 'CPU Scheduler does not own an I/O Priority writer' `
    -Pattern 'previous_io_priority|applied_io_priority|set_io_priority|NtSetInformationProcess' `
    -Paths @(
        'src/features/winderust_features/cpu_scheduler.rs',
        'src/features/winderust_features/cpu_scheduler/process_control.rs'
    )

Assert-NoUnexpectedWriter `
    -Mechanism 'GPU scheduling priority' `
    -ApiPattern 'D3DKMTSetProcessSchedulingPriorityClass' `
    -AllowedLocationPattern '^src\\(?:backend\\crash_recovery|platform\\windows\\gpu_priority)\.rs:'

Assert-SourceMatchCount `
    -Boundary 'single GPU Priority production adapter call' `
    -Pattern 'D3DKMTSetProcessSchedulingPriorityClass\s*\(' `
    -Paths @('src/platform/windows/gpu_priority.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'GPU Priority controller owns transactions without raw WDK' `
    -Pattern 'D3DKMT(?:Get|Set)ProcessSchedulingPriorityClass|D3DKMT_SCHEDULINGPRIORITYCLASS|windows_sys|unsafe' `
    -Paths @('src/control/gpu_priority.rs')

Assert-NoSourceMatch `
    -Boundary 'legacy GPU Priority feature restore owner' `
    -Pattern 'fn apply_once\(|fn current_priority\(|struct AdjustedProcess|struct ProcessHandle|D3DKMT(?:Get|Set)ProcessSchedulingPriorityClass|record_process_change' `
    -Paths @('src/features/priority_control/gpu_priority.rs')

Assert-NoSourceMatch `
    -Boundary 'Process List does not own GPU Priority mutation or restoration' `
    -Pattern 'gpu_priority::apply_once|gpu_priority::current_priority|current_gpu_priority\(' `
    -Paths @('src/ui/process_list.rs')

Assert-NoSourceMatch `
    -Boundary 'CPU Scheduler does not own a GPU Priority writer' `
    -Pattern 'previous_gpu_priority|applied_gpu_priority|set_gpu_priority|D3DKMTSetProcessSchedulingPriorityClass' `
    -Paths @(
        'src/features/winderust_features/cpu_scheduler.rs',
        'src/features/winderust_features/cpu_scheduler/process_control.rs'
    )

Assert-NoUnexpectedWriter `
    -Mechanism 'Processor Affinity (Hard)' `
    -ApiPattern 'SetProcessAffinityMask' `
    -AllowedLocationPattern '^src\\(?:backend\\crash_recovery|platform\\windows\\cpu_allocation)\.rs:'

Assert-NoUnexpectedWriter `
    -Mechanism 'CPU Sets (Soft)' `
    -ApiPattern 'SetProcessDefaultCpuSets' `
    -AllowedLocationPattern '^src\\(?:backend\\crash_recovery|platform\\windows\\cpu_allocation)\.rs:'

Assert-SourceMatchCount `
    -Boundary 'single Processor Affinity production adapter call' `
    -Pattern 'SetProcessAffinityMask\s*\(' `
    -Paths @('src/platform/windows/cpu_allocation.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'single CPU Sets production adapter call' `
    -Pattern 'SetProcessDefaultCpuSets\s*\(' `
    -Paths @('src/platform/windows/cpu_allocation.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'CPU allocation coordinator owns arbitration without raw Windows APIs' `
    -Pattern 'GetProcessAffinityMask|SetProcessAffinityMask|GetProcessDefaultCpuSets|SetProcessDefaultCpuSets|GetSystemCpuSetInformation|SYSTEM_CPU_SET_INFORMATION|windows_sys|unsafe' `
    -Paths @('src/control/cpu_allocation.rs')

Assert-NoSourceMatch `
    -Boundary 'CPU allocation Windows adapter imports no policy layer' `
    -Pattern 'crate::(?:control|features|foreground|rules|ui)' `
    -Paths @('src/platform/windows/cpu_allocation.rs')

Assert-NoSourceMatch `
    -Boundary 'legacy CPU allocation feature mutation or restore owner' `
    -Pattern 'SetProcessAffinityMask|SetProcessDefaultCpuSets|record_(?:affinity|cpu_sets)_change|previous_(?:affinity|cpu_sets)|applied_(?:affinity|cpu_sets)|adjusted_process_ids|struct AffinityAdjustment|struct AdjustedProcess' `
    -Paths @(
        'src/features/cpu_control/cpu_allocation.rs',
        'src/features/cpu_control/cpu_limiter.rs',
        'src/features/winderust_features/cpu_scheduler.rs',
        'src/features/winderust_features/cpu_scheduler/process_control.rs'
    )

Assert-NoUnexpectedWriter `
    -Mechanism 'Job Object information' `
    -ApiPattern 'SetInformationJobObject' `
    -AllowedLocationPattern '^src\\(?:backend\\crash_recovery|platform\\windows\\(?:job|suspension))\.rs:'

Assert-SourceMatchCount `
    -Boundary 'single App Suspension production adapter call' `
    -Pattern 'SetInformationJobObject\s*\(' `
    -Paths @('src/platform/windows/suspension.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'single foreign-job test UI restriction adapter call' `
    -Pattern 'SetInformationJobObject\s*\(' `
    -Paths @('src/platform/windows/job.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'shared Job Object adapter imports no policy layer' `
    -Pattern 'crate::(?:control|features|foreground|rules|ui)' `
    -Paths @('src/platform/windows/job.rs')

Assert-SourceMatchCount `
    -Boundary 'single App Suspension freeze-layout contract' `
    -Pattern 'struct JobObjectFreezeInformation' `
    -Paths @('src/platform/windows/suspension.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'App Suspension controller owns lifecycle without raw Job Object APIs' `
    -Pattern 'CreateJobObjectW|AssignProcessToJobObject|IsProcessInJob|SetInformationJobObject|SetLastError|JobObjectFreezeInformation|JOB_OBJECT_FREEZE_INFORMATION_CLASS|windows_sys' `
    -Paths @('src/control/suspension.rs')

Assert-SourceMatchCount `
    -Boundary 'single shared suspension handle thread-safety contract' `
    -Pattern 'unsafe impl Send for WindowsSuspensionHandle' `
    -Paths @('src/control/suspension.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'App Suspension Windows adapter imports no policy layer' `
    -Pattern 'crate::(?:control|features|foreground|rules|ui)' `
    -Paths @('src/platform/windows/suspension.rs')

Assert-NoUnexpectedWriter `
    -Mechanism 'CPU Limiter waitable timer' `
    -ApiPattern 'CreateWaitableTimerExW|SetWaitableTimer|CancelWaitableTimer|WaitForMultipleObjects' `
    -AllowedLocationPattern '^src\\platform\\windows\\cpu_limiter\.rs:'

Assert-NoUnexpectedWriter `
    -Mechanism 'CPU Limiter thread suspension' `
    -ApiPattern '(?:PssCaptureSnapshot|PssWalkSnapshot|SuspendThread|ResumeThread)\s*(?:,|\()' `
    -AllowedLocationPattern '^src\\(?:backend\\crash_recovery|platform\\windows\\thread_suspension)\.rs:'

Assert-NoSourceMatch `
    -Boundary 'CPU Limiter policy uses only the typed thread-suspension adapter' `
    -Pattern '(?:PssCaptureSnapshot|PssWalkSnapshot|SuspendThread|ResumeThread)\s*\(' `
    -Paths @(
        'src/control/cpu_limiter.rs',
        'src/control/cpu_limiter/thread_fallback.rs',
        'src/features/cpu_control/cpu_limiter.rs'
    )

Assert-NoSourceMatch `
    -Boundary 'CPU Limiter controller owns timing without raw Win32' `
    -Pattern 'CreateWaitableTimerExW|SetWaitableTimer|CancelWaitableTimer|WaitForMultipleObjects|windows_sys|unsafe' `
    -Paths @('src/control/cpu_limiter.rs')

Assert-NoSourceMatch `
    -Boundary 'App Suspension feature and UI layers do not own mutation or restoration' `
    -Pattern 'SetInformationJobObject|ProcessFreezer|record_suspended_job|forget_suspended_job|app_suspension_(?:freeze_requests|process_requests)' `
    -Paths @(
        'src/features/advanced_controls/app_suspension.rs',
        'src/ui/app_suspension.rs',
        'src/ui/process_list.rs',
        'src/backend/automation.rs',
        'src/backend/automation/status.rs'
    )

Assert-NoUnexpectedWriter `
    -Mechanism 'Timer Resolution' `
    -ApiPattern 'timeBeginPeriod|timeEndPeriod' `
    -AllowedLocationPattern '^src\\platform\\windows\\timer_resolution\.rs:'

Assert-SourceMatchCount `
    -Boundary 'single Timer Resolution begin adapter call' `
    -Pattern 'unsafe \{ time_begin_period\s*\(' `
    -Paths @('src/platform/windows/timer_resolution.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'single Timer Resolution end adapter call' `
    -Pattern 'unsafe \{ time_end_period\s*\(' `
    -Paths @('src/platform/windows/timer_resolution.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'single Timer Resolution platform contract' `
    -Pattern 'trait TimerResolutionPlatform' `
    -Paths @('src/platform/windows/timer_resolution.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'Timer Resolution controller owns lifecycle without raw WinMM' `
    -Pattern 'timeGetDevCaps|timeBeginPeriod|timeEndPeriod|link\(name = "winmm"\)' `
    -Paths @('src/control/timer_resolution.rs')

Assert-NoSourceMatch `
    -Boundary 'Timer Resolution feature remains policy-only' `
    -Pattern 'timeGetDevCaps|timeBeginPeriod|timeEndPeriod|impl Drop' `
    -Paths @('src/features/advanced_controls/timer_resolution.rs')

Assert-NoSourceMatch `
    -Boundary 'Timer Resolution remains process-lifetime state outside crash recovery' `
    -Pattern 'RecoveryIntent|ProcessValue|record_process_change|baseline|managed' `
    -Paths @('src/control/timer_resolution.rs')

Assert-SourceMatchCount `
    -Boundary 'single shared process-control mutation-acquisition OpenProcess adapter call' `
    -Pattern 'unsafe \{ OpenProcess\s*\(' `
    -Paths @('src/platform/windows/process.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'single shared process-control access contract' `
    -Pattern 'enum ProcessAccess' `
    -Paths @('src/platform/windows/process.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'typed process-control validation owns no raw Win32 acquisition' `
    -Pattern 'windows_sys|unsafe|GetCurrentProcessId|PROCESS_[A-Z_]+' `
    -Paths @('src/control/process.rs')

Assert-NoSourceMatch `
    -Boundary 'shared process-control adapter imports no feature or control policy' `
    -Pattern 'crate::(?:control|features|foreground|rules|ui)' `
    -Paths @('src/platform/windows/process.rs')

Assert-NoUnexpectedWriter `
    -Mechanism 'Memory Trim' `
    -ApiPattern 'SetProcessWorkingSetSize' `
    -AllowedLocationPattern '^src\\platform\\windows\\memory_trim\.rs:'

Assert-NoUnexpectedWriter `
    -Mechanism 'process termination' `
    -ApiPattern 'TerminateProcess' `
    -AllowedLocationPattern '^src\\platform\\windows\\process_termination\.rs:'

Assert-SourceMatchCount `
    -Boundary 'single process termination production adapter call' `
    -Pattern 'TerminateProcess\s*\(' `
    -Paths @('src/platform/windows/process_termination.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'process termination controller owns batch semantics without raw Win32' `
    -Pattern 'TerminateProcess|windows_sys|unsafe' `
    -Paths @('src/control/process_termination.rs')

Assert-NoSourceMatch `
    -Boundary 'process query and UI layers do not own termination mutation' `
    -Pattern 'TerminateProcess|fn terminate_process|fn terminate_process_trees' `
    -Paths @(
        'src/foreground/process_list.rs',
        'src/ui/process_list.rs'
    )

Assert-NoSourceMatch `
    -Boundary 'process termination remains irreversible and outside recovery ownership' `
    -Pattern 'RecoveryIntent|ProcessValue|record_process_change|baseline|managed|impl Drop' `
    -Paths @('src/control/process_termination.rs')

Assert-NoUnexpectedWriter `
    -Mechanism 'power-plan writes' `
    -ApiPattern 'Power(SetActiveScheme|DuplicateScheme|DeleteScheme|WriteACValueIndex|WriteDCValueIndex|WriteFriendlyName|WriteDescription)' `
    -AllowedLocationPattern '^src\\platform\\windows\\power_plan\.rs:'

Assert-SourceMatchCount `
    -Boundary 'single active power-plan production adapter call' `
    -Pattern 'PowerSetActiveScheme\s*\(' `
    -Paths @('src/platform/windows/power_plan.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'single adaptive power-plan duplicate adapter call' `
    -Pattern 'PowerDuplicateScheme\s*\(' `
    -Paths @('src/platform/windows/power_plan.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'single power-plan deletion adapter call' `
    -Pattern 'PowerDeleteScheme\s*\(' `
    -Paths @('src/platform/windows/power_plan.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'single A/C processor-setting adapter call' `
    -Pattern 'PowerWriteACValueIndex\s*\(null_mut' `
    -Paths @('src/platform/windows/power_plan.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'single battery processor-setting adapter call' `
    -Pattern 'PowerWriteDCValueIndex\s*\(null_mut' `
    -Paths @('src/platform/windows/power_plan.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'power-plan domain facade owns no raw Windows APIs' `
    -Pattern 'windows_sys|unsafe|PowerSetActiveScheme|PowerDuplicateScheme|PowerDeleteScheme|PowerWriteACValueIndex|PowerWriteDCValueIndex|PowerWriteFriendlyName|PowerWriteDescription|PowerReadACValueIndex|PowerReadDCValueIndex|PowerEnumerate|PowerGetActiveScheme|PowerRegisterForEffectivePowerModeNotifications' `
    -Paths @('src/power/powercfg.rs')

Assert-NoSourceMatch `
    -Boundary 'power-plan Windows adapter imports no policy or application layer' `
    -Pattern 'crate::(?:application|control|features|foreground|rules|ui)' `
    -Paths @('src/platform/windows/power_plan.rs')

Assert-NoSourceMatch `
    -Boundary 'UI contains no automatic power-plan decision or recovery mutation' `
    -Pattern 'record_power_plan_change|\bdecide\(|power::[^;]*\bset_active\b' `
    -Paths @('src/ui')

Assert-SourceMatchCount `
    -Boundary 'Advanced Power Plan Tuning applies through one staged application-service adapter' `
    -Pattern 'apply_processor_power_values_staged\(' `
    -Paths @('src/application/advanced_power_plan_tuning.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'UI does not own Advanced Power Plan Tuning persistence' `
    -Pattern 'apply_processor_power_values|read_processor_power_values|read_plan_personality' `
    -Paths @('src/ui')

Assert-NoSourceMatch `
    -Boundary 'persistent Advanced Power Plan Tuning stays outside recovery ownership' `
    -Pattern 'RecoveryIntent|record_power_plan_change|baseline|managed|impl Drop' `
    -Paths @('src/application/advanced_power_plan_tuning.rs')

Assert-SourceMatchCount `
    -Boundary 'ordinary power-plan decisions reconcile through RuntimeCore once' `
    -Pattern 'let\s+decision\s*=\s*decide\(' `
    -Paths @('src/backend/automation/runner.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'PowerPlanController owns the production recovery-journal switch adapter' `
    -Pattern 'record_power_plan_change\(original_guid, expected_guid\)' `
    -Paths @('src/control/power_plan.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'UI does not own input-hook lifecycle' `
    -Pattern '\bInputHook\b|InputHook::install' `
    -Paths @('src/ui')

Assert-SourceMatchCount `
    -Boundary 'RuntimeHandle owns the single input-hook installation site' `
    -Pattern 'InputHook::install\(' `
    -Paths @('src/backend/automation.rs') `
    -ExpectedCount 1

Assert-NoUnexpectedWriter `
    -Mechanism 'Win32 Priority Separation registry writes' `
    -ApiPattern 'write_registry_dword_(root|create_root)' `
    -AllowedLocationPattern '^src\\(?:application\\win32_priority_separation|backend\\win_registry)\.rs:'

Assert-SourceMatchCount `
    -Boundary 'single Win32 Priority Separation machine-write adapter call' `
    -Pattern 'write_registry_dword_root\(' `
    -Paths @('src/application/win32_priority_separation.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'single Win32 Priority Separation backup-write adapter call' `
    -Pattern 'write_registry_dword_create_root\(' `
    -Paths @('src/application/win32_priority_separation.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'UI does not own Win32 Priority Separation registry access' `
    -Pattern 'HKEY_LOCAL_MACHINE|write_registry_dword_(?:root|create_root)|(?:read|write|ensure)_win32_priority_separation(?:_backup|_with_status)?\(' `
    -Paths @('src/ui')

Assert-NoSourceMatch `
    -Boundary 'persistent Win32 Priority Separation stays outside recovery ownership' `
    -Pattern 'RecoveryIntent|record_process_change|record_power_plan_change|baseline|managed|impl Drop' `
    -Paths @('src/application/win32_priority_separation.rs')

Assert-NoUnexpectedWriter `
    -Mechanism 'startup registration' `
    -ApiPattern 'create_subkey\(RUN_KEY\)|set_value\(VALUE_NAME|delete_value\(VALUE_NAME' `
    -AllowedLocationPattern '^src\\backend\\startup\.rs:'

Assert-SourceMatchCount `
    -Boundary 'startup registration is applied through SettingsEditor once' `
    -Pattern 'startup::set_startup_with_windows\(' `
    -Paths @('src/application/settings.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'UI does not own settings coordination or startup registration' `
    -Pattern 'SettingsCoordinator|SettingsDraft|set_startup_with_windows' `
    -Paths @('src/ui')

Assert-NoUnexpectedWriter `
    -Mechanism 'persisted settings I/O' `
    -ApiPattern 'config::storage::(load|save|import_toml_from|export_toml_to)' `
    -AllowedLocationPattern '^src\\application\\settings\.rs:'

$runtimeObservationConsumers = @(
    'src/backend/automation.rs',
    'src/backend/automation/runner.rs',
    'src/features'
)
Assert-NoSourceMatch `
    -Boundary 'runtime feature consumers use CycleObservations instead of raw process/window collectors' `
    -Pattern '(^|[^A-Za-z0-9_.])(list_processes(_with_paths)?|foreground_process(_id)?|visible_window_process_ids|top_level_window_process_ids|process_from_id)\(' `
    -Paths $runtimeObservationConsumers

Assert-NoSourceMatch `
    -Boundary 'outer automation deadlines are owned by RefreshScheduler' `
    -Pattern 'let\s+mut\s+next_' `
    -Paths @('src/backend/automation.rs')

Assert-SourceMatchCount `
    -Boundary 'one pass-local CycleObservations instance is created by the worker loop' `
    -Pattern 'CycleObservations::default\(\)' `
    -Paths @('src/backend/automation.rs') `
    -ExpectedCount 1

# Iced owns UI state and subscriptions; RuntimeHandle remains the sole automation facade.
Assert-SourceMatchCount `
    -Boundary 'Process List enumeration and confirmed tree refresh stay independent of RuntimeCore observations' `
    -Pattern 'list_processes_with_paths\(\)' `
    -Paths @('src/ui/process_list.rs') `
    -ExpectedCount 2

Assert-SourceMatchCount `
    -Boundary 'Process List owns one resource sampling call' `
    -Pattern 'sample_process_resources\(' `
    -Paths @('src/ui/process_list.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'WinderustApp owns one Process Catalog candidate collection' `
    -Pattern '^\s*candidates:\s*Vec<String>,' `
    -Paths @('src/ui/app.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'WinderustApp owns one Process List read model' `
    -Pattern '^\s*processes:\s*process_list::ProcessList,' `
    -Paths @('src/ui/app.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'WinderustApp does not mirror Process List resources selection or sorting' `
    -Pattern '^\s*(?:process_resource_samples|process_resource_usage|hide_inaccessible|expanded_groups|selected_process_id|process_details)\s*:' `
    -Paths @('src/ui/app.rs')

Assert-NoSourceMatch `
    -Boundary 'runtime and control layers do not depend on UI models or Iced' `
    -Pattern 'crate::ui|\biced::|\bgpui::|ProcessCatalogModel|ProcessListModel|DashboardModel|UpdateModel|ShellModel' `
    -Paths @('src/backend/automation.rs', 'src/backend/automation', 'src/control')

Assert-SourceMatchCount `
    -Boundary 'WinderustApp owns one Home read model' `
    -Pattern '^\s*home:\s*home::Model,' `
    -Paths @('src/ui/app.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'WinderustApp does not mirror individual dashboard histories' `
    -Pattern '^\s*(?:cpu_usage|cpu_usage_history|memory_usage|memory_usage_history|io_usage|io_usage_history|network_usage|network_usage_history)\s*:' `
    -Paths @('src/ui/app.rs')

Assert-NoSourceMatch `
    -Boundary 'Home model keeps native sampling handles on its worker' `
    -Pattern '^\s*\w+:\s*(?:CpuUsageMonitor|IoUsageMonitor|NetworkUsageMonitor)|unsafe impl Send|Mutex<(?:CpuUsageMonitor|IoUsageMonitor|NetworkUsageMonitor)' `
    -Paths @('src/ui/home.rs')

Assert-SourceMatchCount `
    -Boundary 'WinderustApp owns one settings and update UI editor' `
    -Pattern '^\s*preferences:\s*settings_pages::Editor,' `
    -Paths @('src/ui/app.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'WinderustApp does not mirror update response fields' `
    -Pattern '^\s*(?:available_update|latest_version|update_check_in_progress|update_check_message|startup_update_modal_visible|startup_update_modal_closing)\s*:' `
    -Paths @('src/ui/app.rs')

Assert-SourceMatchCount `
    -Boundary 'Iced application owns one current navigation page' `
    -Pattern '^\s*page:\s*Page,' `
    -Paths @('src/ui/app.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'navigation remains a UI projection without Windows mutation or runtime lifecycle' `
    -Pattern 'RuntimeHandle|RuntimeCore|windows_sys|unsafe|crate::control|crate::platform' `
    -Paths @('src/ui/navigation.rs')

Assert-SourceMatchCount `
    -Boundary 'WinderustApp retains one segmented runtime status snapshot' `
    -Pattern '^\s*status:\s*RuntimeStatusSnapshot,' `
    -Paths @('src/ui/app.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'runtime snapshot retains one shared feature-status segment' `
    -Pattern '^\s*pub feature_status:\s*Arc<RuntimeFeatureStatus>,' `
    -Paths @('src/backend/automation.rs') `
    -ExpectedCount 1

$icedApplication = Get-Content -LiteralPath 'src/ui/app.rs' -Encoding utf8 -Raw
if ($icedApplication -notmatch '(?s)struct WinderustApp\s*\{[^}]*\bsettings:\s*SettingsEditor,') {
    throw 'WinderustApp must retain SettingsEditor as its settings composition boundary.'
}
Write-Host 'Architecture ownership gate passed: WinderustApp owns SettingsEditor'


Assert-NoSourceMatch `
    -Boundary 'WinderustApp does not mirror individual runtime feature snapshots' `
    -Pattern '^\s*(?:background_efficiency_status|app_suspension_status|cpu_limiter_status|cpu_sets_soft_status|processor_affinity_hard_status|by_running_app_status|cpu_scheduler_status|process_priority_status|thread_priority_status|dynamic_priority_boost_status|io_priority_status|gpu_priority_status|memory_priority_status|memory_trim_status|timer_resolution_status)\s*:' `
    -Paths @('src/ui/app.rs')

Assert-NoSourceMatch `
    -Boundary 'production Iced renderer contains no GPUI entities or framework dependencies' `
    -Pattern '\bgpui(?:_component)?\b|\bEntity<|\bContext<Self>' `
    -Paths @('src/ui')

Assert-NoSourceMatch `
    -Boundary 'Iced process controls do not mutate native state outside runtime commands' `
    -Pattern 'SetPriorityClass|SetProcessInformation|SetProcessAffinityMask|SetProcessDefaultCpuSets|SuspendThread|ResumeThread|TerminateProcess|unsafe' `
    -Paths @('src/ui/process_list.rs', 'src/ui/process_details.rs', 'src/ui/cpu_allocation.rs', 'src/ui/adaptive_engine.rs')

Assert-SourceMatchCount `
    -Boundary 'single RuntimeHandle lifecycle facade' `
    -Pattern '^pub struct RuntimeHandle\s*\{' `
    -Paths @('src/backend/automation.rs') `
    -ExpectedCount 1

Assert-SourceMatchCount `
    -Boundary 'single RuntimeCore automation composition root' `
    -Pattern '^pub\(super\) struct RuntimeCore\s*\{' `
    -Paths @('src/backend/automation/runner.rs') `
    -ExpectedCount 1

Assert-NoSourceMatch `
    -Boundary 'legacy automation owner names are deleted' `
    -Pattern 'BackgroundAutomation|HiddenAutomationRunner' `
    -Paths @('src')

Assert-NoSourceMatch `
    -Boundary 'legacy Process List restoration closures are deleted' `
    -Pattern 'process_quick_action_restore|process_efficiency_mode_overrides|quick_apply_(?:process|thread|io|gpu|memory)_priority' `
    -Paths @('src/ui')

Assert-NoSourceMatch `
    -Boundary 'feature and UI layers do not own crash-recovery protocol state' `
    -Pattern 'RecoveryEntry|RecoveryCommand|RecoveryIntent|record_(?:process|thread_priority|power_plan|suspended_job)_change' `
    -Paths @('src/features', 'src/ui')

Write-Host 'All architecture ownership gates passed.'
