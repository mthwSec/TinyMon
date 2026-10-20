TinyMon - user guide
====================

TinyMon is a small always-on-top window, the widget, that shows how busy your
PC is: the processor, memory, graphics, disk, network and more, each with a
line of its last minute. It needs no admin rights and almost no resources.

The full manual, new versions and the source code:
https://github.com/mthwSec/TinyMon


Starting and quitting
---------------------

- Start TinyMon from the Start menu. The first time, the widget appears in the
  top-right corner of the main screen, and TinyMon's icon appears in the
  notification area (the tray, next to the clock).
- Only one TinyMon runs at a time: starting it again brings the widget back.
- To quit, right-click the widget or the tray icon and choose Exit. Esc and
  Alt+F4 only hide the widget; TinyMon keeps running in the tray.


Using the widget
----------------

- Move it: drag it from anywhere. TinyMon remembers the spot.
- Menu: right-click the widget or the tray icon.
- Show or hide it: left-click the tray icon, or use the menu's Hide / Show.

The menu:

  Hide / Show          hides the widget to the tray, or shows it again
  Settings...          opens the settings window
  Lock (click-through) the mouse goes through the widget to what is below
                       it; unlock it from the tray icon's menu
  Move to corner       puts the widget in a corner of its screen
  Light theme          a light widget instead of the dark one
  Start with Windows   starts TinyMon when you sign in to Windows
  Exit                 saves the settings and quits


What the rows mean
------------------

  CPU        how busy the processor is
  RAM        memory in use: percent, then used/total GB
  GPU        how busy each graphics chip is, one row per chip, by its name
  VRAM       graphics memory in use ("GPU Mem" on built-in graphics)
  C0, C1...  each processor core (Per-core CPU)
  Disk R/W   disk reads and writes per second
  Net        data sent (up arrow) and received (down arrow) per second
  Up         time since Windows started
  Bat        battery charge and the time left, or "chg" while charging
  Temp       temperatures, see below

- The line under a value shows its last minute or so.
- The labels of CPU, RAM, GPU, VRAM, the cores and Temp turn red at 90% or
  more (90 degrees C for a temperature).
- "n/a" means this PC does not give that reading. "..." means TinyMon is
  still taking its first measurement (about a second).
- "+2" next to the title means 2 rows did not fit on the screen. They are the
  last ones in your list; move important rows up in Settings.

CPU, RAM and GPU are on at first. Turn on the others in Settings.


Settings
--------

Open them from the menu: Settings...

- Metrics: tick a metric to show it; the up and down arrows change the order.
- Appearance: how see-through the widget is (opacity), how often it updates
  (update interval) and the size of its text (font size).
- Hotkeys: two optional keyboard shortcuts, one to show or hide the widget
  and one to open the settings. Click a field and press the keys, for example
  Ctrl+Alt+M; Clear removes the shortcut. Avoid a plain letter or number
  without Ctrl or Alt: other programs would stop receiving that key.
- The preview on the right shows the widget as you change things.
- OK or Apply saves your changes; Cancel drops them. Reset fills in the
  defaults, which you then save with OK or Apply.


Temperature
-----------

Temperature is off at first; turn it on in Settings.

- It shows the temperature of a graphics card whose driver reports one, as
  most NVIDIA and AMD graphics cards do. Built-in graphics usually report
  none.
- It also shows the temperature sensors that the PC's firmware publishes to
  Windows (labelled "ACPI"), once their reading changes. Many desktop PCs
  publish only a fixed value, which TinyMon does not show.
- Windows gives normal programs no processor temperature. Reading one needs a
  special driver, which TinyMon does not install. So on many desktop PCs
  without a graphics card the row reads "Temp n/a".


Starting with Windows
---------------------

Setup asks whether TinyMon should start when you sign in (it is ticked by
default). You can change it at any time with the menu's Start with Windows,
or in Task Manager's Startup tab.


Where the settings are kept
---------------------------

TinyMon saves its settings in tinymon.cfg, a small text file next to
tinymon.exe (installed, that is %LOCALAPPDATA%\Programs\TinyMon). If that
folder cannot be written to, the file goes to %APPDATA%\TinyMon instead.
TinyMon rewrites the file whenever it saves, so edit it only while TinyMon is
not running. The full manual lists its keys.


Updating
--------

Download the new setup from the website and run it. It closes TinyMon,
replaces it, keeps your settings and can start it again at the end.


Uninstalling
------------

Open Windows Settings, Apps, find TinyMon and choose Uninstall. It closes
TinyMon, removes the Start with Windows entry and asks whether to delete your
settings too. If you keep them, installing TinyMon again brings them back.


The portable version (zip)
--------------------------

The zip's tinymon.exe runs from any folder, a USB stick too, without being
installed. It keeps its settings next to it. To remove it, turn off Start
with Windows in its menu, quit it, and delete the folder.


Warnings on the first run
-------------------------

TinyMon is not signed with a certificate, so Windows SmartScreen may say
"Windows protected your PC". Choose More info, then Run anyway.

Some antivirus programs flag small unsigned programs like TinyMon by mistake.
TinyMon makes no network connections, and the only things it writes are its
settings file and, if you turn it on, the Start with Windows entry.


Requirements
------------

Windows 10 version 1703 or later, or Windows 11, 64-bit. Nothing else needs
to be installed.


License
-------

TinyMon is free software under the MIT license or the Apache License 2.0,
at your option: see the LICENSE-MIT and LICENSE-APACHE files.
