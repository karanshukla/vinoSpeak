//! The window as a media player on the session bus (MPRIS), so the desktop's
//! media controls and the keyboard's media keys show and drive the reading.
//! The bus thread only passes commands to the window and shows what it is told.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result};
use zbus::interface;
use zbus::object_server::SignalEmitter;
use zbus::{ObjectPath, OwnedVariant as OwnedValue, Variant as Value};

const NAME: &str = "org.mpris.MediaPlayer2.vinospeak";
const PATH: &str = "/org/mpris/MediaPlayer2";
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
/// How often the bus thread looks for a change to show.
const POLL: Duration = Duration::from_millis(100);

/// What the desktop asked the player to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Play,
    Pause,
    PlayPause,
    Stop,
}

/// What MPRIS calls the player's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    Playing,
    Paused,
    #[default]
    Stopped,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Status::Playing => "Playing",
            Status::Paused => "Paused",
            Status::Stopped => "Stopped",
        }
    }
}

/// What the desktop is shown: the reading's state, its id, the file's name
/// and the voice reading it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Playing {
    pub status: Status,
    pub id: u64,
    pub title: String,
    pub voice: String,
}

struct Root;

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {}

    fn quit(&self) {}

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn identity(&self) -> &str {
        "vinoSpeak"
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        "dev.vinospeak.Listen"
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

struct Player {
    commands: mpsc::Sender<Command>,
    wake: Box<dyn Fn() + Send + Sync>,
    now: Playing,
}

impl Player {
    fn send(&self, command: Command) {
        if self.commands.send(command).is_ok() {
            (self.wake)();
        }
    }
}

fn value(v: Value<'_>) -> OwnedValue {
    v.try_into().expect("a plain value has no file descriptor")
}

/// MPRIS metadata for what is playing; only a track id when nothing is
/// (pinned by `mpris::tests::the_metadata_names_the_file_and_the_voice`).
fn metadata(now: &Playing) -> HashMap<String, OwnedValue> {
    let mut out = HashMap::new();
    if now.status == Status::Stopped {
        let none = ObjectPath::from_static_str_unchecked(NO_TRACK);
        out.insert("mpris:trackid".to_owned(), value(none.into()));
        return out;
    }
    let track = format!("/dev/vinospeak/Listen/reading/{}", now.id);
    let track = ObjectPath::try_from(track).expect("digits make a valid path");
    out.insert("mpris:trackid".to_owned(), value(track.into()));
    out.insert("xesam:title".to_owned(), value(now.title.as_str().into()));
    let artist = vec![now.voice.clone()];
    out.insert("xesam:artist".to_owned(), value(artist.into()));
    out
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn play(&self) {
        self.send(Command::Play);
    }

    fn pause(&self) {
        self.send(Command::Pause);
    }

    fn play_pause(&self) {
        self.send(Command::PlayPause);
    }

    fn stop(&self) {
        self.send(Command::Stop);
    }

    fn next(&self) {}

    fn previous(&self) {}

    fn seek(&self, _offset: i64) {}

    fn set_position(&self, _track: ObjectPath<'_>, _position: i64) {}

    fn open_uri(&self, _uri: &str) {}

    #[zbus(property)]
    fn playback_status(&self) -> &str {
        self.now.status.name()
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        metadata(&self.now)
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    /// The reading's position is in text, not time, so none is given.
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        0
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

/// The player on the bus; dropping it takes it off.
pub struct Media {
    shown: mpsc::Sender<Playing>,
    last: Playing,
}

impl Media {
    /// On the session bus, its commands sent to `commands` with `wake`
    /// called after each; None when there is no session bus.
    pub fn start(
        commands: mpsc::Sender<Command>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Option<Self> {
        let (shown, changes) = mpsc::channel();
        let (started, result) = mpsc::channel();
        let player = Player {
            commands,
            wake: Box::new(wake),
            now: Playing::default(),
        };
        std::thread::Builder::new()
            .name("mpris".to_owned())
            .spawn(move || zbus::block_on(serve(player, changes, started)))
            .ok()?;
        match result.recv() {
            Ok(Ok(())) => Some(Self {
                shown,
                last: Playing::default(),
            }),
            Ok(Err(e)) => {
                eprintln!("vinospeak-gui: no media controls: {e:#}");
                None
            }
            Err(_) => None,
        }
    }

    /// Show `now` on the bus, if it changed.
    pub fn show(&mut self, now: Playing) {
        if now != self.last {
            let _ = self.shown.send(now.clone());
            self.last = now;
        }
    }
}

async fn connect(player: Player) -> Result<zbus::Connection> {
    zbus::connection::Builder::session()
        .name(NAME)
        .serve_at(PATH, Root)
        .serve_at(PATH, player)
        .build()
        .await
        .context("taking the media player's name on the session bus")
}

/// Serve until the window drops its `Media`, showing each change it sends.
async fn serve(
    player: Player,
    changes: mpsc::Receiver<Playing>,
    started: mpsc::Sender<Result<()>>,
) {
    let conn = match connect(player).await {
        Ok(conn) => conn,
        Err(e) => {
            let _ = started.send(Err(e));
            return;
        }
    };
    let _ = started.send(Ok(()));
    let Ok(player) = conn.object_server().interface::<_, Player>(PATH).await else {
        return;
    };
    loop {
        let mut latest = None;
        loop {
            match changes.try_recv() {
                Ok(now) => latest = Some(now),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }
        if let Some(now) = latest {
            player.get_mut().await.now = now;
            let emitter: &SignalEmitter<'_> = player.signal_emitter();
            let shown = player.get().await;
            let _ = shown.playback_status_changed(emitter).await;
            let _ = shown.metadata_changed(emitter).await;
        }
        async_io::Timer::after(POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On the session bus, when there is one (none in CI) and no window
    /// holds the name already.
    #[test]
    fn the_desktop_drives_the_player_and_reads_its_state() {
        let (tx, commands) = mpsc::channel();
        let Some(mut media) = Media::start(tx, || {}) else {
            eprintln!("skipped: no session bus, or the name is taken");
            return;
        };
        media.show(Playing {
            status: Status::Paused,
            id: 1,
            title: "notes.md".to_owned(),
            voice: "Heart".to_owned(),
        });
        zbus::block_on(async {
            let conn = zbus::Connection::session().await.unwrap();
            let player = zbus::Proxy::new(&conn, NAME, PATH, "org.mpris.MediaPlayer2.Player")
                .await
                .unwrap();
            player.call_method("PlayPause", &()).await.unwrap();
            let mut status = String::new();
            for _ in 0..30 {
                status = player.get_property("PlaybackStatus").await.unwrap();
                if status == "Paused" {
                    break;
                }
                async_io::Timer::after(POLL).await;
            }
            assert_eq!(status, "Paused");
        });
        assert_eq!(
            commands.recv_timeout(Duration::from_secs(5)),
            Ok(Command::PlayPause)
        );
    }

    #[test]
    fn the_metadata_names_the_file_and_the_voice() {
        let now = Playing {
            status: Status::Playing,
            id: 3,
            title: "notes.md".to_owned(),
            voice: "Heart".to_owned(),
        };
        let shown = metadata(&now);
        assert_eq!(
            shown["xesam:title"],
            value(Value::from("notes.md")),
            "{shown:?}"
        );
        assert_eq!(
            shown["mpris:trackid"],
            value(
                ObjectPath::try_from("/dev/vinospeak/Listen/reading/3")
                    .unwrap()
                    .into()
            )
        );
        let stopped = metadata(&Playing::default());
        assert_eq!(stopped.len(), 1);
        assert_eq!(
            stopped["mpris:trackid"],
            value(ObjectPath::try_from(NO_TRACK).unwrap().into())
        );
    }
}
