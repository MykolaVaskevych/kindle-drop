fn main() {
    // Give the Windows .exe its icon (shown in Explorer, the taskbar and the
    // installer's shortcuts).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("packaging/windows/kindle-drop.ico")
            .compile()
            .expect("embed Windows resources");
    }
}
