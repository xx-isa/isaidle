# isaidle

Wayland's `ext-idle-notify` listener inspired by
[swayidle](https://github.com/swaywm/swayidle) with `logind` integration, and
locked screen aware timers.

It's stupidly simple and mostly vibe coded but i might come in and clean it up.

Currently only compatible with [Niri window manager](https://github.com/niri-wm/niri). There are no plans to add support for other window managers, if it's too hard to implement.

## Usage

1. clone
2. build
3. run

optionally: use the systemd service file in `resources/` to run isaidle as a system service

## Configuration

There's no config file or command line options. Just fork it, babes.

## Why

It's a learning project, both in AI and Rust/General Programming. And for shits and giggles.
