# Changelog

## [0.14.0](https://github.com/Casperjuel/sponos/compare/v0.13.0...v0.14.0) (2026-11-13)


### Features

* "What should play next?" under the playing song suggests follow-up songs with a Play next button ([7bbacf1](https://github.com/Casperjuel/sponos/commit/7bbacf1c3d11495140c5c81dc3e7ef739064613f))
* inspiration, describe the music you want and get a playlist ([7bbacf1](https://github.com/Casperjuel/sponos/commit/7bbacf1c3d11495140c5c81dc3e7ef739064613f))
* manage rooms in one place, tick rooms in and out of a group with per-room volume, or drag a room onto another in the sidebar ([7bbacf1](https://github.com/Casperjuel/sponos/commit/7bbacf1c3d11495140c5c81dc3e7ef739064613f))


### Bug Fixes

* covers load from another speaker when the group's lead speaker is slow to answer ([7bbacf1](https://github.com/Casperjuel/sponos/commit/7bbacf1c3d11495140c5c81dc3e7ef739064613f))
* keep scrollbars clear of the rounded panel corners and show them on hover ([0816350](https://github.com/Casperjuel/sponos/commit/081635061b2eb65dd231981851979630db949d13))
* play button no longer touches the progress bar (glass theme) ([7bbacf1](https://github.com/Casperjuel/sponos/commit/7bbacf1c3d11495140c5c81dc3e7ef739064613f))
* rooms in a group keep their order instead of reshuffling every minute ([7bbacf1](https://github.com/Casperjuel/sponos/commit/7bbacf1c3d11495140c5c81dc3e7ef739064613f))
* Spotify setup no longer dead-ends when you are not on the built-in app's user list, and the first steps are clearer ([7bbacf1](https://github.com/Casperjuel/sponos/commit/7bbacf1c3d11495140c5c81dc3e7ef739064613f))
* the cover shrinks on small windows so nothing under the song is cut off ([7bbacf1](https://github.com/Casperjuel/sponos/commit/7bbacf1c3d11495140c5c81dc3e7ef739064613f))

## [0.13.0](https://github.com/Casperjuel/sponos/compare/v0.12.0...v0.13.0) (2026-10-09)


### Features

* new app icon, a vinyl record with glowing green grooves and an S on the label ([ce5087d](https://github.com/Casperjuel/sponos/commit/ce5087da9bd17625980abb7909146e63da44de03))

## [0.12.0](https://github.com/Casperjuel/sponos/compare/v0.11.0...v0.12.0) (2026-10-09)


### Features

* now playing as a vinyl record that spins while playing ([7b02b4f](https://github.com/Casperjuel/sponos/commit/7b02b4f2a126d49424b887d47e365a6f491b78aa))
* records swap with an outro and intro animation when the song changes ([1b93d43](https://github.com/Casperjuel/sponos/commit/1b93d436ca582f4820f4fabf326d168008657eb4))


### Bug Fixes

* slower record spin ([803f56b](https://github.com/Casperjuel/sponos/commit/803f56b0a3e490e54deedef00dce293ffb15e00e))
* the cover's reflection no longer smears the song title ([635a8b7](https://github.com/Casperjuel/sponos/commit/635a8b7f3970c4af7c181d00c0e7b605699827aa))

## [0.11.0](https://github.com/Casperjuel/sponos/compare/v0.10.0...v0.11.0) (2026-10-09)


### Features

* new reactions pop and float up in the queue ([c615fde](https://github.com/Casperjuel/sponos/commit/c615fde3c0b359ffaf0fc0d5b1f3549527202a29))


### Bug Fixes

* download links no longer point at a release whose files are still building ([767fea3](https://github.com/Casperjuel/sponos/commit/767fea363fc47654b11cfeed76f7e61c75eb90e1))

## [0.10.0](https://github.com/Casperjuel/sponos/compare/v0.9.0...v0.10.0) (2026-10-09)


### Features

* react to songs with 💩 🦄 🔥 💃 😴 🎉, next to the thumbs ([46f96f3](https://github.com/Casperjuel/sponos/commit/46f96f3394b5c09ede1b9ce45ce2ef074ead2c04))
* turn crossfade on or off from the top bar ([d6169ae](https://github.com/Casperjuel/sponos/commit/d6169ae8401d09dfd5daf5db1f7b4ff6b437c684))


### Bug Fixes

* votes and "added by" stopped saving once a system had any (weak ETag from the blob store) ([49f2970](https://github.com/Casperjuel/sponos/commit/49f2970ac8cfdab74028b4dfe234147456f5f614))

## [0.9.0](https://github.com/Casperjuel/sponos/compare/v0.8.0...v0.9.0) (2026-10-09)


### Features

* songs go poof when you clear the queue ([5538a94](https://github.com/Casperjuel/sponos/commit/5538a94464b8a0dbdefbf6fe85e375a1b9b5d7a6))


### Bug Fixes

* Clear queue did nothing because the app window has no confirm dialog ([5538a94](https://github.com/Casperjuel/sponos/commit/5538a94464b8a0dbdefbf6fe85e375a1b9b5d7a6))
* removing the floorplan image asks with a second click instead of a dialog that never showed ([5538a94](https://github.com/Casperjuel/sponos/commit/5538a94464b8a0dbdefbf6fe85e375a1b9b5d7a6))

## [0.8.0](https://github.com/Casperjuel/sponos/compare/v0.7.0...v0.8.0) (2026-10-07)


### Features

* richer About page, Check for Updates and Settings in the app menu ([bf09a37](https://github.com/Casperjuel/sponos/commit/bf09a3725936687f1bcf41faac88a193847a13cc))
* zoom and pan the floorplan, with an Organize speakers mode for moving them ([cb7b003](https://github.com/Casperjuel/sponos/commit/cb7b003409dff56cd53351d9dca93e077f0ce174))

## [0.7.0](https://github.com/Casperjuel/sponos/compare/v0.6.0...v0.7.0) (2026-10-07)


### Features

* guided Spotify setup that walks you through creating your own Spotify app ([b80346d](https://github.com/Casperjuel/sponos/commit/b80346df081bbbd0536c1fe77c0480fe89c2acbf))


### Bug Fixes

* clearer wording for the Spotify app in Settings ([66b602c](https://github.com/Casperjuel/sponos/commit/66b602cb12eb6da9083b1bc18241810e67e4356a))
* Spotify in Settings is simply connected or not ([0b19218](https://github.com/Casperjuel/sponos/commit/0b192187d8af1acd223a1cc942b1f2c4728ce150))

## [0.6.0](https://github.com/Casperjuel/sponos/compare/v0.5.0...v0.6.0) (2026-10-07)


### Features

* set up your own Spotify app from Settings to log in without the 5-user limit ([5a034d4](https://github.com/Casperjuel/sponos/commit/5a034d4912affa3a0c021608f2c99d065abd8a24))


### Bug Fixes

* explain when a Spotify account isn't registered for the app, and log it out ([5a034d4](https://github.com/Casperjuel/sponos/commit/5a034d4912affa3a0c021608f2c99d065abd8a24))

## [0.5.0](https://github.com/Casperjuel/sponos/compare/v0.4.0...v0.5.0) (2026-10-07)


### Features

* paste a Spotify playlist link to open it and keep it in your library ([34edc98](https://github.com/Casperjuel/sponos/commit/34edc980e8d32988ede820ae44914f9a8e5dfb00))
* search Spotify without logging in ([34edc98](https://github.com/Casperjuel/sponos/commit/34edc980e8d32988ede820ae44914f9a8e5dfb00))

## [0.4.0](https://github.com/Casperjuel/sponos/compare/v0.3.0...v0.4.0) (2026-10-07)


### Features

* button to jump to the playing song in long queues ([0f23887](https://github.com/Casperjuel/sponos/commit/0f238874c913fc27e002ad15a6aad286b225d773))
* show who added a song (Spotify avatar and name) in the queue and on the now-playing screen ([0f23887](https://github.com/Casperjuel/sponos/commit/0f238874c913fc27e002ad15a6aad286b225d773))
* vote songs up or down, shared with everyone on the Sonos system ([0f23887](https://github.com/Casperjuel/sponos/commit/0f238874c913fc27e002ad15a6aad286b225d773))


### Bug Fixes

* explain why macOS asks for local network access ([a819de0](https://github.com/Casperjuel/sponos/commit/a819de0a3da866f324c7bd13a1bed6c17b86e2dc))
* icon buttons in the top bar no longer squeeze into ovals ([0f23887](https://github.com/Casperjuel/sponos/commit/0f238874c913fc27e002ad15a6aad286b225d773))
* mini player play button stays round and the volume slider stays inside the window ([130d6e2](https://github.com/Casperjuel/sponos/commit/130d6e2d0c8f7f2cacbb1a1951d9245125340f77))
* Spotify login in release builds used an empty client ID ([a819de0](https://github.com/Casperjuel/sponos/commit/a819de0a3da866f324c7bd13a1bed6c17b86e2dc))

## [0.3.0](https://github.com/Casperjuel/sponos/compare/v0.2.0...v0.3.0) (2026-10-07)


### Features

* automatic updates from GitHub releases ([3bcdf7b](https://github.com/Casperjuel/sponos/commit/3bcdf7bdfc773ae0160fcd8369d3c41397511178))
* share floorplan, system name and Spotify link with everyone on the same Sonos system ([b6e5075](https://github.com/Casperjuel/sponos/commit/b6e50759c6a0fcee77d4382f9c170bce0eb1517c))
* sync service for sharing floorplans per Sonos system ([79181e5](https://github.com/Casperjuel/sponos/commit/79181e55308c858cc570d7d7a2ef56b78c0235ce))


### Bug Fixes

* rename the app binary and packages to sponos ([8e08b56](https://github.com/Casperjuel/sponos/commit/8e08b56d2c73a042b56fa16ed0193f7ccfd285e8))

## [0.2.0](https://github.com/Casperjuel/sonos-ui/compare/v0.1.0...v0.2.0) (2026-10-07)


### Features

* new Sponos app icon and name ([604e19b](https://github.com/Casperjuel/sonos-ui/commit/604e19b005e361638ce6babac89730ac3e41c44c))


### Bug Fixes

* pass GITHUB_TOKEN to the release upload ([685b465](https://github.com/Casperjuel/sonos-ui/commit/685b4655d472bb28303acb8259e282fb53536706))

## 0.1.0 (2026-10-07)


### Features

* Sonos controller with Spotify search, queue, mini player, floorplan and multi-system support ([9f381bd](https://github.com/Casperjuel/sonos-ui/commit/9f381bd4181faafb6b54f8b2c424cda93cb22eed))
