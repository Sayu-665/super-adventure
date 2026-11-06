# Parry Arena (Luduvo)

A parry-based melee arena for [Luduvo](https://luduvo.com), built from two Roblox files:

- **Parry Based Combat System X Movement System** (`.rbxl`): the combat rules, movement and tuning numbers
- **Combat VFX Pack #1** (`.rbxm`): every effect, rebuilt for Luduvo

Luduvo uses Luau like Roblox, but its API is different (ECS components, EventTables instead of
RemoteEvents, no ParticleEmitter, no Trail, no Humanoid animations from Roblox). Nothing could be
copied over directly, so this is a **rewrite for Luduvo**. The mechanics and the numbers come from
the original configs. The code and the effects are new.

```
ParryArena/
  scripts/ArenaServer.server.lua   server script: arena, combat rules, dummies, AI, movement
  scripts/ArenaClient.client.lua   client script: input, VFX, HUD, dummy limbs, camera punch
  tests/                           checks you can run without Luduvo (see Testing)
```

## Setting it up in Luduvo Studio

1. Create a new place. Any template is fine, because the server script builds the whole arena when the game starts.
2. Add a Part and name it `ArenaServer`. In the Outliner, attach a **Server** script to it and
   replace the script with the contents of `scripts/ArenaServer.server.lua`.
3. Add a second Part, `ArenaClient`, attach a **Client** script and paste in
   `scripts/ArenaClient.client.lua`. Luduvo allows one script per Instance, which is why the two
   scripts go on two Parts.
4. Optional: make both Parts anchored and invisible, and move them out of the way. The scripts
   don't use the Parts' positions.
5. Press Play. Press **H** to show or hide the controls.

If the place has no `PlayerSpawner`, the server adds one, and it also places three spawn points
on the spawn plaza.

## Controls

| Input | Action |
| --- | --- |
| LMB | Attack: a 4-hit combo, M1 to M4. Hold the button to keep swinging. |
| F | Block. **Tap it as a hit lands to parry.** |
| R | Critical: slow, heavy, and it breaks a held guard. |
| RMB | Feint your critical before it swings |
| E | Weapon skill: Barrage (Fists), Whirlwind (Blade), Volley (Rapier) |
| Q | Dash in your WASD direction. Getting hit early in a dash triggers a **perfect dodge**. |
| Ctrl | Slide while running. Press Space during a slide to leap. |
| Space | Jump, double jump, slide leap, or wall jump |
| 1 / 2 / 3 | Switch to Fists / Blade / Rapier |
| Tab | Lock on |
| H | Show or hide the controls panel |

Chat commands for testing: `/diff weak|normal|hard|very hard|impossible`, `/weapon fists|blade|rapier`, `/hitboxes`.

## The arena

- **Training row**: four stone dummies, one for each skill you practise:
  - *Training Dummy*: takes hits, for practising the combo.
  - *Blocking Dummy*: always blocks, so you can watch posture build. A Critical breaks its guard.
  - *Parry Dummy*: parries everything that comes from the front, then counters. Parry the counter back, or get behind it.
  - *Attacking Dummy*: swings on a steady rhythm, for practising parry timing.
- **Duel circle**: the AI Duelist. To set its difficulty, step on one of the five pads behind it
  (Weak / Normal / Hard / Very Hard / Impossible). The presets are the original AI settings.
- **Wall-run corridor** (east): two tall walls for wall runs and wall jumps.
- **Slide lane** (west): a long strip with a ramp at the end for slide leaps.
- **VFX showcase** (north-east): the seven coloured platforms from the VFX pack, at their
  original sizes and in their original order. Stand on one and its effect family plays on loop:
  Hard Hit, Parry, Dash/Flashstep, Shockwave, Perfect Block, Stars/Drops/Rocks, Trails/Swing.

## The combat rules (from the original configuration)

- **Combo**: M1 to M4, chained within 1 s. Each attack lasts 0.35 s, then there is a 0.2 s
  cooldown (0.55 s after M4). M4 knocks the target back 7 studs.
- **Parry**: your block press is compared with the moment the attacker swung. It counts if you
  pressed up to 0.07 s early or 0.16 s late (0.19 s for criticals).
  - A successful parry stuns the attacker for 0.71 s and stops them attacking for 0.9 s.
  - A *missed* parry (the hit lands within 0.35 s of your press) drops your guard and stops you
    attacking for 0.75 s.
  - Tapping block still counts as a parry attempt for 0.3 s after the press.
- **Block and posture**: each blocked hit adds 31 posture, a blocked skill hit adds 92, and a
  Critical fills it completely. A parry also adds 18. At 100 the guard breaks: 8.4 damage and a
  1.25 s stun. Posture decays at 1.6/s after a 1.6 s pause, or after 4 s if it is above 80.
- **Damage**: M1 2.8 / M2 2.8 / M3 3.5 / M4 4.9 / Critical 9.8. Skills do less per hit but hit many times.
- **Stamina**: max 100. A Critical costs 22 and a skill costs 6. Hits drain the victim's stamina
  and refill the attacker's. It regenerates at 19/s, or 7/s in combat.
- **Dash**: 135 speed for 0.25 s with 0.12 s of i-frames. A hit inside the first 0.18 s is a
  perfect dodge: a 200-speed boost and 0.3 s more i-frames.
- **Slide**: needs a speed of at least 10. It adds an 18 boost (capped at 75), loses 20/s to
  friction, and you can steer it. The leap carries the slide speed plus 40 forward and 60 up.
- **Wall run**: 45 speed along the wall for up to 2.5 s. The wall jump pushes 14 out, 54 along the wall and 28 up.
- **Weapons**: Fists (walk speed 26), Blade (23), Rapier (24, with a long thrusting hitbox).

All of these values are in the `CONFIG`, `DAMAGE`, `WEAPONS` and `AI_*` tables at the top of
`ArenaServer.server.lua`.

## How the VFX pack was rebuilt

Luduvo has no particle emitters, and the pack's textures are Roblox assets, so every effect is
rebuilt from pooled Parts, Spheres and Wedges. They are moved, scaled and faded each frame by a
small particle system in the client script. Each effect keeps the colours, sizes, lifetimes,
speeds and emit counts read from the `.rbxm`:

| Pack effect | Used for | Built from |
| --- | --- | --- |
| Hit VFX #1 / #2 | normal hits | flash, ink flash, two rings, line streaks, cubes, light |
| Hard Hit #1 / #3 + Drops + Blood Hit | M4 and Critical hits | red flash layers, rings, long streaks, drops, blood droplets |
| Parry #1 / #2 (#ffb06a / #ffa745) | parries | glow, ripples, star lines, dots, cubes, crescent arcs, strong light |
| Block VFX (#54aaff) | blocked hits, held guard | shield plate, impact rings, drops, plus a shield plane while blocking |
| Block Break (#ff3434) | guard breaks | shattering shield, 25 wedge shards, ground ring |
| Perfect Block (#aa00ff) | perfect dodges | purple impact rings, flys, shards, afterimage, screen flash |
| Swing VFX #1 to #4 | M1 to M4 | slash arcs: flat R→L, flat L→R, diagonal, overhead |
| Flashstep / Smoke / Dash Smoke | dashes | ground smoke puff, crescent, black and white afterimages |
| Shockwave + Rock Debris | criticals, showcase | ground rings, flash, rocks, dirt |
| Stars Burst | guard break stun | yellow bubble plus orbiting stars |
| Trail #1 / #2 | dash, slide, leap, wall run | a ribbon of fading segments |

Damage numbers rise and fall like the original's damage indicator. A small FOV kick and camera
shake play on hits that involve you.

## What is not ported, and why

- **Animations**: the Roblox animation assets can't be loaded in Luduvo. Swing and hit timings
  that came from animation markers are now fixed numbers (`PlayerSwingDelay`, `PlayerHitDelay`).
  Players get slash arcs instead of attack animations. The dummies' limbs are posed by script.
- **Sounds**: they were Roblox asset IDs. `game.Sound.PlayAt` exists in Luduvo if you want to add your own.
- Ragdolls, air combat juggles, uppercut, down slam, air slam, wall climb, vaults, crouch and
  crawl, swing bars, ProfileStore saving, Cmdr commands and mobile/gamepad controls are not
  ported. Heavy hits use a knockback launch instead of a ragdoll.
- **Latency compensation**: Luduvo has no ping API, so the parry check shifts block presses back
  by a flat 0.05 s (`LatencyCompensation`).
- **Perfect dodge desaturation**: there are no post-processing effects, so a purple screen flash stands in for it.

## Testing

I could not run Luduvo itself while building this, so **the game has not been played in the real engine yet.**
Here is what was checked:

- `tests/luaucheck.py` parses both scripts (a Luau subset). It reports syntax errors, undefined
  names, and locals used before they are defined.
- `tests/run_harness.py` runs both scripts together against `tests/mock_luduvo.lua`. The mock
  implements Luduvo's API with vectors, EventTables, prefabs, raycasts, input, players and the
  rotation library. Every property and method name is checked against Luduvo's published type
  definitions. The harness then plays 17 scripted scenarios: combo hits, parrying the attacking
  dummy, an early block counting as a block rather than a parry, a critical guard break, the
  parry dummy's counter, a perfect dodge, slide and leap, double jump, wall run and wall jump,
  the difficulty pads, all 7 showcase platforms, weapon swaps and skills, the AI duel, a golem
  dying and respawning, and a feint. All 17 pass, with no runtime errors and no API members
  outside Luduvo's definitions.

```
pip install lupa
git clone --depth 1 https://github.com/luduvo-devhub/luduvo-lsp
cd ParryArena/tests
python luaucheck.py ../scripts/ArenaServer.server.lua ../scripts/ArenaClient.client.lua
python run_harness.py ../scripts luduvo-lsp/scripts/luduvo/api-docs/en-us
```

The mock can't tell how Luduvo actually behaves at runtime. These are the things to check first in Studio:

- Character velocity writes (dash, slide, knockback) are applied server side. If the character
  controller overrides them, the bursts will feel weak.
- Client scripts spawn their VFX parts with `game.Prefabs.Spawn`. If that is blocked on clients,
  you will see the warning `this client cannot create parts, VFX are off`.
- UI properties such as `UIPosition` / `UISize` and the `TextXAlignment` values, used for the HUD.
- The forward direction, taken as the model's −Z axis, is used for hitboxes, blocking direction and the dummies.

Every engine call is wrapped so that a missing feature only turns off one piece and doesn't stop
the game. Warnings start with `[ParryArena]` in the output.

## Credits

The combat design and tuning numbers are taken from *Parry Based Combat System X Movement System*
by skaterstudios. The effect designs are taken from *Combat VFX Pack #1*. Neither product's
source code nor its assets are included in this repository. Both are licensed products, so check
their licenses before you publish a game based on them.
