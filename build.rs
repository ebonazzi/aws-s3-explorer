fn main() {
    #[cfg(target_os = "windows")]
    {
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile()
            .expect("failed to embed Windows icon resource");
    }
}
