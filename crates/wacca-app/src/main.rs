//! The `lily` executable; the game is the `wacca_app` library (shared with the iOS app).

fn main() -> std::process::ExitCode {
    wacca_app::main()
}
