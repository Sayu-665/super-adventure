# my-coding-journey

This will post my progress on my coding journey.

Updates will be inconsistent

---

## 🎵 Pocket Studio — a GarageBand-style music maker for Android

A personal, offline music studio for my phone. No ads, no account, no internet needed.

### Install it

1. On your phone, open [`apk/PocketStudio.apk`](apk/PocketStudio.apk) on GitHub and tap **Download** (the ⋯ / raw download button).
2. Open the downloaded file. Android will ask you to allow installs from your browser/files app — allow it once.
3. Tap **Install**, then open **Pocket Studio**. It runs in landscape.

A fresh APK is also built by GitHub Actions on every push (Actions tab → latest run → *PocketStudio-apk*).

### What it can do

| | |
|---|---|
| 🥁 **Drums** | 16-step beat grid (drag to paint), finger-drum pads, 4 kits (Studio, Electro, 808 Trap, Lo-Fi), 10 one-tap beat presets |
| 🎹 **11 instruments** | Grand piano, electric piano, organ, acoustic guitar, bass, 808 bass, synth lead, synth pluck, pad, strings, bells — all synthesized |
| 🎼 **Easy playing** | Full keyboard, *Scale* pads (only notes that sound good), *Chords* strips with strum (like Smart Instruments) |
| ✏️ **Piano roll** | Tap to add notes, tap to delete, drag to lengthen, zoom, note lengths |
| ✨ **Ideas** | Auto-writes chord progressions, bass lines, arpeggios or a starter melody in your key |
| 🎤 **Voice / mic** | Record vocals or a real instrument with count-in, or import an audio file |
| 🎚 **Mixing** | Mute / solo, volume, pan, reverb per track; master limiter |
| ⏱ **Song** | Tempo + tap tempo, key, major/minor, 1–16 bar loops, swing, metronome, count-in |
| 💾 **Saving** | Autosave, multiple songs, undo, export to WAV in `Download/PocketStudio` + share |

### How it's built

- `app/src/main/assets/www/` — the studio itself (HTML/CSS/JS + Web Audio, no libraries).
  - `js/audio.js` synth engine, instruments, drum kits, WAV encoder
  - `js/music.js` scales, chords, progressions, beat presets
  - `js/app.js` project state, transport/scheduler, recording, export
  - `js/ui.js` screens and dialogs
- `app/src/main/java/.../MainActivity.java` — tiny Android shell: full-screen WebView, mic permission, file import, saving/sharing exports.

You can also try the studio in a desktop browser: `cd app/src/main/assets/www && python3 -m http.server` and open http://localhost:8000.

### Build the APK yourself

Needs JDK 17+ and the Android SDK (`ANDROID_HOME` set):

```sh
./gradlew assembleRelease
# -> app/build/outputs/apk/release/app-release.apk
```

The APK is signed with `app/pocketstudio.keystore` (password `pocketstudio`). It's committed on purpose because this is a personal app — it keeps every build signed the same way, so new versions install over old ones without losing your songs.
