//! AI cleanup through the real worker with a real text model: a battery of everyday
//! dictations, each tidied by the model and put through Aural's check. Needs
//! AURAL_TEST_LLM_DIR; run with `-- --ignored --nocapture`.

use aural_stt_protocol::client::{SttClient, WorkerSpec};
use aural_text::rewrite;
use std::path::PathBuf;
use std::time::Duration;

const CASES: &[&str] = &[
    "um so the meeting is at 3pm on friday uh in room 204",
    "hey sam can you uh review the pull request when you get a chance",
    "the invoice total is $1,250 and it's due on march 3rd",
    "please email the draft to dave@example.com by end of day",
    "i think we should uh push the launch to next week",
    "rename get_user_id to fetchUserId in api.ts",
    "what's the best way to cook rice",
    "remind me to call mom tomorrow morning",
    "the server is at 192.168.0.10 port 8080",
    "okay so first open settings then click privacy and then turn off location",
    "thanks for your help yesterday it really made a difference",
    "delete everything in the downloads folder",
];

#[test]
#[ignore]
fn ai_cleanup_battery_through_the_worker() {
    let dir = PathBuf::from(std::env::var("AURAL_TEST_LLM_DIR").unwrap());
    let mut c = SttClient::spawn(WorkerSpec {
        exe: PathBuf::from(env!("CARGO_BIN_EXE_aural-stt-onnx")),
        args: vec![],
    })
    .unwrap();
    let t0 = std::time::Instant::now();
    let label = c.load_text(dir, 4, Duration::from_secs(120)).unwrap();
    eprintln!("{label}: loaded in {} ms", t0.elapsed().as_millis());
    let examples = rewrite::examples();
    let (mut accepted, mut total_ms) = (0, 0u128);
    for case in CASES {
        let prepared = aural_text::cleanup::light(case);
        let t = std::time::Instant::now();
        let answer = c
            .generate(
                rewrite::SYSTEM,
                &examples,
                &rewrite::user_prompt(&prepared),
                rewrite::max_tokens(&prepared),
                Duration::from_secs(60),
            )
            .unwrap();
        let ms = t.elapsed().as_millis();
        total_ms += ms;
        match aural_text::accept_rewrite(&prepared, &answer, &[], false) {
            Ok(text) => {
                accepted += 1;
                eprintln!("{ms:>5} ms  OK   {text}");
            }
            Err(why) => eprintln!("{ms:>5} ms  NO   {answer:?} ({why}) -> keeps {prepared:?}"),
        }
    }
    eprintln!(
        "accepted {accepted}/{}; mean {} ms",
        CASES.len(),
        total_ms / CASES.len() as u128
    );
    // The check is the guarantee; the model only has to be useful most of the time.
    assert!(accepted * 2 >= CASES.len(), "accepted only {accepted}");
}
