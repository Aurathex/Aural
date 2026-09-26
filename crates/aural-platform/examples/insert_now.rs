//! Developer tool: after a delay, deliver text to whatever window is in front, exactly
//! as Aural does after a dictation, and print what happened. Used to test insertion
//! into real apps without speaking.
//!
//!   cargo run -p aural-platform --example insert_now -- 3 "Testing Aural, one two three."

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let delay: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(3);
    let text = args
        .next()
        .unwrap_or_else(|| "Testing Aural, one two three.".into());
    std::thread::sleep(std::time::Duration::from_secs(delay));
    let hwnd = aural_platform::insert::foreground_window();
    let target = aural_platform::insert::foreground_target();
    println!(
        "target: {} / {} (hwnd {hwnd})",
        target.process, target.class
    );
    let t0 = std::time::Instant::now();
    let outcome = aural_platform::insert::insert(&text, hwnd)?;
    println!("outcome: {outcome:?} in {} ms", t0.elapsed().as_millis());
    Ok(())
}
