//! Le Mac ne se met pas en veille pendant un enregistrement ou un traitement : le cœur dit quand
//! il faut le garder éveillé (l'interface en fait une assertion de non-veille de macOS). Pendant
//! un enregistrement, l'écran reste allumé aussi : éteint, il n'est plus capturé.

mod bench;

use bench::Bench;
use greffier_core::fake::FakeResources;
use greffier_core::{Command, Wake, Settings, Status};

const RECORDING: Option<Wake> = Some(Wake { reason: "Greffier enregistre une réunion", screen: true });
const PROCESSING: Option<Wake> = Some(Wake { reason: "Greffier traite une réunion", screen: false });

#[test]
fn mac_stays_awake_during_a_recording_and_not_after() {
    let mut bench = Bench::new();
    bench.execute(Command::ChangeSettings(Settings { process_on_stop: false, ..Settings::default() }));
    assert_eq!(bench.greffier.keep_mac_awake(), None);

    bench.execute(Command::StartRecording);
    bench.advance(2.0);
    assert_eq!(bench.greffier.keep_mac_awake(), RECORDING);

    bench.finish_recording();
    assert_eq!(bench.greffier.keep_mac_awake(), None);
}

#[test]
fn mac_stays_awake_from_start_to_end_of_processing() {
    let mut bench = Bench::new();
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    // Dans la file, l'étape démarre au prochain pas : la veille est déjà empêchée.
    assert_eq!(bench.greffier.keep_mac_awake(), PROCESSING);

    bench.advance_until(id, |s| matches!(s, Status::Running { .. }));
    assert_eq!(bench.greffier.keep_mac_awake(), PROCESSING);

    bench.advance_until(id, |s| *s == Status::Done);
    assert_eq!(bench.greffier.keep_mac_awake(), None);
}

#[test]
fn recording_takes_priority_while_the_queue_waits() {
    let mut bench = Bench::new();
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.execute(Command::StartRecording);
    bench.advance(1.0);
    assert_eq!(bench.greffier.keep_mac_awake(), RECORDING);
}

#[test]
fn a_stopped_or_failed_processing_lets_the_mac_sleep() {
    let mut bench = Bench::new();
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.advance(0.5);
    bench.execute(Command::StopProcessing(id));
    assert_eq!(bench.greffier.keep_mac_awake(), None);

    let mut a = bench::adapters();
    a.resources = Box::new(FakeResources::missing(1.0));
    let mut bench = Bench::with(a);
    let id = bench.meeting_to_process();
    bench.execute(Command::Process(id));
    bench.advance_until(id, |s| matches!(s, Status::Failed { .. }));
    assert_eq!(bench.greffier.keep_mac_awake(), None);
}
