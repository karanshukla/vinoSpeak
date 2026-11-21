//! The window as a media player on the session bus (MPRIS), so the desktop's
//! media controls and the keyboard's media keys show and drive the reading.
//! The bus thread only passes commands to the window and shows what it is told.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use zbus::interface;
use zbus::object_server::SignalEmitter;
use zbus::{ObjectPath, OwnedVariant as OwnedValue, Variant as Value};

const NAME: &str = "org.mpris.MediaPlayer2.vinospeak";
const PATH: &str = "/org/mpris/MediaPlayer2";
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
/// How often the bus thread looks for a change to show.
const POLL: Duration = Duration::from_millis(100);
/// The app's icon, shown as the reading's art.
const ART: &[u8] = include_bytes!("../../docs/assets/vinospeak.svg");
/// How far the position may stray from where a desktop counting on from the
/// last one would put it before it is told again: progress moves in steps.
const DRIFT: Duration = Duration::from_secs(2);

/// What the desktop asked the player to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    Play,
    Pause,
    PlayPause,
    Stop,
    /// Read from this far into the text, 0 to 1.
    SeekTo(f32),
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

/// What the desktop is shown: the reading's state, its id, the file's name,
/// the voice reading it, and how long it is estimated to take.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Playing {
    pub status: Status,
    pub id: u64,
    pub title: String,
    pub voice: String,
    pub length: Duration,
}

fn micros(d: Duration) -> i64 {
    i64::try_from(d.as_micros()).unwrap_or(i64::MAX)
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
    /// Microseconds into the reading, as the window last said.
    position: Arc<AtomicI64>,
    art: Option<String>,
}

impl Player {
    fn send(&self, command: Command) {
        if self.commands.send(command).is_ok() {
            (self.wake)();
        }
    }

    /// Read from `position` microseconds in, if that is within the reading.
    fn seek_to(&self, position: i64) {
        let length = micros(self.now.length);
        if self.now.status != Status::Stopped && length > 0 {
            let at = position.clamp(0, length) as f64 / length as f64;
            self.send(Command::SeekTo(at as f32));
        }
    }
}

/// The art written where the desktop can read it, as a file URL.
fn art_url() -> Option<String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty())?;
    let path = PathBuf::from(dir).join("vinospeak-gui-art.svg");
    std::fs::write(&path, ART).ok()?;
    Some(format!("file://{}", path.display()))
}

fn value(v: Value<'_>) -> OwnedValue {
    v.try_into().expect("a plain value has no file descriptor")
}

fn track_id(id: u64) -> String {
    format!("/dev/vinospeak/Listen/reading/{id}")
}

/// MPRIS metadata for what is playing; only a track id when nothing is
/// (pinned by `mpris::tests::the_metadata_names_the_file_and_the_voice`).
fn metadata(now: &Playing, art: Option<&str>) -> HashMap<String, OwnedValue> {
    let mut out = HashMap::new();
    if now.status == Status::Stopped {
        let none = ObjectPath::from_static_str_unchecked(NO_TRACK);
        out.insert("mpris:trackid".to_owned(), value(none.into()));
        return out;
    }
    let track = ObjectPath::try_from(track_id(now.id)).expect("digits make a valid path");
    out.insert("mpris:trackid".to_owned(), value(track.into()));
    out.insert("xesam:title".to_owned(), value(now.title.as_str().into()));
    let artist = vec![now.voice.clone()];
    out.insert("xesam:artist".to_owned(), value(artist.into()));
    out.insert("mpris:length".to_owned(), value(micros(now.length).into()));
    if let Some(art) = art {
        out.insert("mpris:artUrl".to_owned(), value(art.into()));
    }
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

    fn seek(&self, offset: i64) {
        self.seek_to(self.position.load(Ordering::Relaxed).saturating_add(offset));
    }

    /// Ignored for a reading that is no longer the one playing, as MPRIS asks.
    fn set_position(&self, track: ObjectPath<'_>, position: i64) {
        if track.as_str() == track_id(self.now.id) {
            self.seek_to(position);
        }
    }

    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    fn open_uri(&self, _uri: &str) {}

    #[zbus(property)]
    fn playback_status(&self) -> &str {
        self.now.status.name()
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        metadata(&self.now, self.art.as_deref())
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

    /// Estimated from how far through the text it is; see `Playing::length`.
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        self.position.load(Ordering::Relaxed)
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
        true
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
    position: Arc<AtomicI64>,
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
        let position = Arc::new(AtomicI64::new(0));
        let player = Player {
            commands,
            wake: Box::new(wake),
            now: Playing::default(),
            position: Arc::clone(&position),
            art: art_url(),
        };
        std::thread::Builder::new()
            .name("mpris".to_owned())
            .spawn(move || zbus::block_on(serve(player, changes, started)))
            .ok()?;
        match result.recv() {
            Ok(Ok(())) => Some(Self {
                shown,
                last: Playing::default(),
                position,
            }),
            Ok(Err(e)) => {
                eprintln!("vinospeak-gui: no media controls: {e:#}");
                None
            }
            Err(_) => None,
        }
    }

    /// Show `now` on the bus, if it changed, `position` into it.
    pub fn show(&mut self, now: Playing, position: Duration) {
        self.position.store(micros(position), Ordering::Relaxed);
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
    let position = Arc::clone(&player.get().await.position);
    let mut told = (0_i64, Instant::now());
    loop {
        let mut latest = None;
        loop {
            match changes.try_recv() {
                Ok(now) => latest = Some(now),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }
        let emitter: &SignalEmitter<'_> = player.signal_emitter();
        let at = position.load(Ordering::Relaxed);
        if let Some(now) = latest {
            player.get_mut().await.now = now;
            let shown = player.get().await;
            let _ = shown.playback_status_changed(emitter).await;
            let _ = shown.metadata_changed(emitter).await;
            told = (at, Instant::now());
            let _ = Player::seeked(emitter, at).await;
        }
        let playing = player.get().await.now.status == Status::Playing;
        let counted = if playing {
            told.0.saturating_add(micros(told.1.elapsed()))
        } else {
            told.0
        };
        if at.abs_diff(counted) > DRIFT.as_micros() as u64 {
            told = (at, Instant::now());
            let _ = Player::seeked(emitter, at).await;
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
        media.show(
            Playing {
                status: Status::Paused,
                id: 1,
                title: "notes.md".to_owned(),
                voice: "Heart".to_owned(),
                length: Duration::from_secs(10),
            },
            Duration::from_secs(4),
        );
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
            let position: i64 = player.get_property("Position").await.unwrap();
            assert_eq!(position, 4_000_000);
            let track = ObjectPath::try_from(track_id(1)).unwrap();
            player
                .call_method("SetPosition", &(track, 7_500_000_i64))
                .await
                .unwrap();
            let other = ObjectPath::try_from(track_id(2)).unwrap();
            player
                .call_method("SetPosition", &(other, 1_i64))
                .await
                .unwrap();
        });
        let asked: Vec<Command> =
            std::iter::from_fn(|| commands.recv_timeout(Duration::from_secs(2)).ok()).collect();
        assert_eq!(asked, [Command::PlayPause, Command::SeekTo(0.75)]);
    }

    #[test]
    fn the_metadata_names_the_file_and_the_voice() {
        let now = Playing {
            status: Status::Playing,
            id: 3,
            title: "notes.md".to_owned(),
            voice: "Heart".to_owned(),
            length: Duration::from_secs(10),
        };
        let shown = metadata(&now, Some("file:///run/art.svg"));
        assert_eq!(shown["mpris:length"], value(10_000_000_i64.into()));
        assert_eq!(shown["mpris:artUrl"], value("file:///run/art.svg".into()));
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
        let stopped = metadata(&Playing::default(), Some("file:///run/art.svg"));
        assert_eq!(stopped.len(), 1);
        assert_eq!(
            stopped["mpris:trackid"],
            value(ObjectPath::try_from(NO_TRACK).unwrap().into())
        );
    }
}
