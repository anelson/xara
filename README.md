# XaraNext

XaraNext 1.0 is a VARA-compatible modem for linux, freebsd and macOS, based
on xara 0.1.3 by x0rro. tested against VARA HF 4.9.0 and FM 4.4.0. it adds
experimental support for the waveform VARA HF 4.9.1 uses from 2027, and for
named keyings of your own (see below).

it is a drop-in replacement: the programs are still called `xara`,
`xara-monitor` and `xara-keygen`, and config files are unchanged.

## minimum config

set your call sign, HF bandwidth or FM mode, audio, and (if using a radio)
radio and ptt. do this with command line flags or a config file.

    xara --call YOURCALL --hf 2300 --audio alsa \
         --capture plughw:CARD=Device,DEV=0 \
         --playback plughw:CARD=Device,DEV=0

to transmit, add `--radio` and `--ptt` flags.

## other features

 - web UI for configuration and management
 - all the usual TCP ports (works with your favorite software)
 - digipeating (enable by call with `--digi N1CALL`)
 - `xara-monitor`: passive VARA HF monitor (with web UI)

## building

install deps (debian/ubuntu):

    apt install libpulse-dev libasound2-dev

then install from here (`xara`, `xara-monitor` and `xara-keygen`):

    cargo install --locked --git https://github.com/CowboyPilot/xara xara

alternatively for a local setup you can do:

    cargo build --release
    cargo test

other platforms are covered in [BUILDING.md](BUILDING.md).

the systemd, udev and sysusers files and `dist/install.sh` are linux-only.

### reproducing a release

release tarballs are reproducible. rebuild one in an `ubuntu:22.04`
container on x86_64:

    # uncomment one of these:
    TARGET=x86_64-unknown-linux-gnu
    # TARGET=aarch64-unknown-linux-gnu
    # TARGET=armv7-unknown-linux-gnueabihf

    apt-get update && apt-get install -y --no-install-recommends ca-certificates git
    git clone -b vVERSION https://github.com/CowboyPilot/xara && cd xara
    scripts/release_deps.sh $TARGET
    . ~/.cargo/env
    scripts/release_bins.sh $TARGET
    scripts/release_tarball.sh HEAD
    sha256sum target/dist/*.tar.xz

## options

    --config FILE           read key = value options from FILE

    --call CALL             your callsign (required)

    --audio BACKEND         pulse, alsa or coreaudio; default pulse on linux/bsd and coreaudio on macOS
    --list-devices          list capture and playback devices for --audio
    --capture SOURCE        audio source
    --playback SINK         audio sink
    --tx-buffer-ms N        TX audio queue, raise on underruns (30-500, default 40)

    --hf 500|2300|2750      HF mode, bandwidth setting
    --fm narrow|wide        FM mode, narrow vs wide

    --radio                 enable radio mode
    --drive-db D            TX drive (<= 0 dB, default -5)
    --ptt [see section]     PTT has a lot of options

    --ui-port N             serve web UI on port N (default off)
    --ui-addr A             webui listen address (default 127.0.0.1)
    --ui-host NAME          accept as HTTP Host for UI (repeatable)
    --ui-token T            UI token (default: random)
    --ui-control            DANGER: remote token-holders can key radio
    --state-dir DIR         web UI settings storage (default: set by systemd)

    --addr A                KISS/command address (default: 127.0.0.1)
    --port N                command port (< 65535, default: 8300)
    --kiss-port N|off       KISS TCP port (default: 8100)
    --persistence P         KISS p-persistence

    --registered            enable registered speeds

    --vara-dir DIR          your VARA HF 4.9.1 install, for the 2027 waveform
    --waveform W            auto, legacy or 2027 (default: auto)

    --digi CALL             FM digipeat for CALL (default: off)
    --record PATH           record rx, tx and PTT to FLAC (or WAV if .wav)
    --log FILE              append UTC-timestamped output to FILE
    --quiet                 only output warnings and errors
    --version and --help    guess

## PTT settings

we support the following PTT methods:

 - rts and dtr on a serial port
 - cm108/cm119 HID GPIO
 - hamlib rigctld CAT
 - vox (not recommended)

you can adjust the following PTT settings as well:

 - PTT lead time
 - PTT tail time
 - max PTT

summary of settings:

    --ptt vox                   use VOX (just don't bother)
    --ptt rts:DEV               key serial port on rts, hold dtr low
    --ptt dtr:DEV               key serial port on dtr, hold rts low
    --ptt cm108:DEV[:GPIO]      cm108/cm119 HID GPIO (default GPIO: 3)
    --ptt cm108:VID:PID[:GPIO]  cm108/cm119 by hex USB id (stable across boots)
    --ptt rigctld[:HOST:PORT]   hamlib rigctld CAT on HOST:PORT

    --ptt-lead-ms N             PTT held for N ms before xmit (default: 0)
    --ptt-tail-ms N             PTT held for N ms after xmit (default: 100)
    --ptt-max-s N               release PTT after N sec max (1-120, default: 30)

## VARA HF 4.9.1 and the 2027 waveform

VARA HF 4.9.1 changes its waveform when the PC clock reaches 2027: new
interleaver and phase tables and a different seed for the control codes.
stations on the old and new waveform cannot hear each other.

the old tables can be generated, so xara builds them itself. the new ones
cannot, and xara does not ship them. to use the new waveform, install VARA HF
4.9.1 yourself and point xara at that directory:

    xara ... --vara-dir "/path/to/VARA"

xara reads `Binary1.dat`, `Binary2.dat`, `Binary3.dat`, `Mod1.dat` and
`Mod2.dat` from there, checks them, and refuses to start if any is missing or
damaged. the waveform is chosen once at startup:

 - `--waveform auto`: old waveform until 2027, new from 2027 (same as VARA)
 - `--waveform legacy`: always the old waveform
 - `--waveform 2027`: always the new waveform (needs `--vara-dir`)

without `--vara-dir` xara stays on the old waveform and warns from 2027.

`--vara-dir` can also point at a named keying made by `xara-keygen` (below).

`xara-monitor` takes the same two options. use `--waveform legacy` to read
an old recording after 2027, and `--waveform 2027` to read a new one before.

status: EXPERIMENTAL. this has not yet been tested on air against VARA HF
4.9.1. FM is not affected by these options. file formats and details are in
[docs/keying.md](docs/keying.md).

## xara-keygen

`xara-keygen` makes table files for a keying of your own, so a group of
stations can share a channel and only react to each other. stations hear each
other only if they loaded the same keying: frames, connect requests and all
other control codes of any other keying (VARA's included) are ignored.

    xara-keygen --name NAME --out DIR

    --name NAME             name of the keying (A-Z, a-z, 0-9, - _ .)
    --out DIR               directory to write (must not hold tables already)

then run every station of the group with `--vara-dir DIR`. a named keying is
used whatever the date; `--waveform legacy` turns it off.

the name is the whole definition: the same name gives the same tables on every
machine, so publishing the name publishes the waveform. this is NOT
encryption and gives no privacy. on amateur bands, publish the name you use.
a named keying cannot talk to VARA or to stations on any other keying. the
exact generator is specified in [docs/keying.md](docs/keying.md).

## xara-monitor

`xara-monitor` listens to VARA HF traffic between other stations and never
transmits. it is considerably more demanding than `xara` and probably won't run
on a pi.

    xara-monitor --audio alsa --capture plughw:CARD=Device,DEV=0

options:

    --wav FILE              load from a wav file
    --stdin RATE            s16le mono at RATE Hz

    --audio BACKEND         pulse, alsa or coreaudio; default coreaudio on macOS, else pulse
    --capture SOURCE        audio source

    --hf 500|2300|2750      HF bandwidth (default 2300)

    --call CALL             known call (repeatable)
    --calls FILE            list of known calls, one per line
    --no-blind              only attempt to resolve known calls

    --vara-dir DIR          your VARA HF 4.9.1 install, for the 2027 waveform
    --waveform W            auto, legacy or 2027 (default: auto)

    --ui-port N             read-only web UI on port N (default: off)
    --ui-addr ADDR          web UI listen addr (default: 127.0.0.1)
    --ui-host NAME          accept as HTTP Host for UI (repeatable)

    --key-idle MIN          max target idle time (default: 30, 0 disables)
    --dump                  dump entire payloads
    --verbose               report unconfirmed blind detections
    --json                  print newline-delimited JSON records
    --log FILE              append UTC-timestamped output to FILE
    --version and --help    guess

## registered mode

run with `xara --registered ...` for higher speeds if you have a registered
copy of VARA. it is off unless you give the flag.

the `registered` feature is part of the default build. to leave it out,
build with `--no-default-features` and name the features you want, e.g.
`--features pulse,alsa`.

## copyright

copyright (c) 2026 x0rro

released under MIT license, see LICENSE
