use std::time::Duration;
fn main() {
    let minute = Duration::from_secs(60);
    dbg!(minute);
    let ffive = minute * 0.5;
    dbg!(ffive);
}
