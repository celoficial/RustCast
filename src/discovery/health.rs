use crate::dlna::av_transport;
use crate::soap::SoapClient;
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// Signal sent from the poll task to the control loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PollSignal {
    Running,
    Paused,
    Resumed,
    Stopped,
    DeviceOffline,
}

const POLL_INTERVAL_SECS: u64 = 3;
const OFFLINE_THRESHOLD: u32 = 3;

/// Spawns a background task that polls GetTransportInfo every 3 seconds.
/// Detects STOPPED, PAUSED_PLAYBACK/PLAYING transitions, and consecutive failures.
/// Returns a join handle and a watch receiver for the current signal.
pub fn spawn_poll_task(
    client: SoapClient,
    av_url: String,
) -> (JoinHandle<()>, watch::Receiver<PollSignal>) {
    let (tx, rx) = watch::channel(PollSignal::Running);

    let handle = tokio::spawn(async move {
        let mut consecutive_errors: u32 = 0;
        let mut last_state = String::from("PLAYING");

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;

            match av_transport::get_transport_state(&client, &av_url).await {
                Ok(state) if state == "STOPPED" => {
                    let _ = tx.send(PollSignal::Stopped);
                    break;
                }
                Ok(state) => {
                    consecutive_errors = 0;
                    if state != last_state {
                        if state == "PAUSED_PLAYBACK" {
                            let _ = tx.send(PollSignal::Paused);
                        } else if state == "PLAYING" && last_state == "PAUSED_PLAYBACK" {
                            let _ = tx.send(PollSignal::Resumed);
                        }
                        last_state = state;
                    }
                }
                Err(e) => {
                    consecutive_errors += 1;
                    eprintln!(
                        "[poll] error ({}/{}): {}",
                        consecutive_errors, OFFLINE_THRESHOLD, e
                    );
                    if consecutive_errors >= OFFLINE_THRESHOLD {
                        let _ = tx.send(PollSignal::DeviceOffline);
                        break;
                    }
                }
            }
        }
    });

    (handle, rx)
}
