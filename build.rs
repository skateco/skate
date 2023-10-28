fn main() {
    print!("{}", std::env::var("TARGET").unwrap());
}