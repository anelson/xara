---
title: XARA-KEYGEN
section: 1
header: User Commands
date: 2026-11-10
---

# NAME

xara-keygen - make the table files for a named HF keying

# SYNOPSIS

**xara-keygen** **--name** *NAME* **--out** *DIR*

# DESCRIPTION

**xara-keygen** writes the table files for a named HF waveform keying. Stations
only hear each other if they load the same keying, so a group can share a
channel and react only to its own members. Load the result in **xara**(1) or
**xara-monitor**(1) with **--vara-dir** *DIR*.

The name is the whole definition: the same name gives the same files on every
machine. A named keying is not encryption and gives no privacy. It cannot
talk to VARA or to stations on any other keying. On amateur bands, publish
the name you use.

# OPTIONS

**--name** *NAME*

:   Name of the keying: 1-64 characters of A-Z, a-z, 0-9, '-', '_' and '.'.
    Case sensitive.

**--out** *DIR*

:   Directory to write, created if needed. It must not already hold table
    files.

**-h**, **--help**, **-V**, **--version**

:   Print the usage or the version and exit.

# FILES

*DIR*/Binary1.dat, Binary2.dat, Binary3.dat, Mod1.dat, Mod2.dat

:   The tables.

*DIR*/xara-keying.txt

:   Name, generator version and control-code seed offset.

# EXIT STATUS

**0**

:   Success.

**1**

:   The files could not be written.

**2**

:   Usage error.

# SEE ALSO

**xara**(1), **xara-monitor**(1)
