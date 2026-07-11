fn main() {
    if let Err(error) = etyma_engine::runtime::run() {
        eprintln!("{error:?}");
        std::process::exit(1);
    }
}
