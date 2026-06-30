fn main() {
    println!("cargo:rerun-if-changed=resources/winderust.ico");

    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".into());
        let mut parts = version
            .split('.')
            .map(|part| part.parse::<u16>().unwrap_or(0))
            .chain(std::iter::repeat(0));
        let file_version = format!(
            "{},{},{},{}",
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
        );
        let icon = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("resources")
            .join("winderust.ico")
            .to_string_lossy()
            .replace('\\', "\\\\");
        let rc = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("winderust.rc");

        std::fs::write(
            &rc,
            format!(
                r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {file_version}
PRODUCTVERSION {file_version}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "Tatsh Siow\0"
            VALUE "FileDescription", "Rust-based Windows tuning controller\0"
            VALUE "FileVersion", "{version}\0"
            VALUE "InternalName", "winderust\0"
            VALUE "OriginalFilename", "winderust.exe\0"
            VALUE "ProductName", "Winderust\0"
            VALUE "ProductVersion", "{version}\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
            ),
        )
        .unwrap();

        embed_resource::compile(&rc, embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}
