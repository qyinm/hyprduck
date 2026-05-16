fn main() {
    if let Err(error) = hyprduck_engine::runtime::run() {
        eprintln!("{error:?}");
        std::process::exit(1);
    }
}
