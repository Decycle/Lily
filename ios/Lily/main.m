// The iOS app is the Rust game (`wacca_app`, linked as a static library): hand it the process.
// winit's event loop runs UIApplicationMain and never returns.
extern int lily_main(void);

int main(int argc, char *argv[]) {
    (void)argc;
    (void)argv;
    return lily_main();
}
