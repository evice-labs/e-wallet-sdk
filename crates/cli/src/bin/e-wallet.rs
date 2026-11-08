#[path = "../main.rs"]
mod main_impl;

fn main() -> anyhow::Result<()> {
    main_impl::main()
}
