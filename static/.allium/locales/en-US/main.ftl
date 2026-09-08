# Launcher
tab-recents = Recents
tab-games = Games
tab-apps = Apps
tab-settings = Settings

sort-alphabetical = Sort: A-Z
sort-last-played = Sort: Recent
sort-most-played = Sort: Playtime
sort-rating = Sort: Rating
sort-release-date = Sort: Release Date
sort-random = Sort: Random
sort-search = Search
sort-favorites = Sort: Favorites
sort-relevance = Sort: Relevance

no-recent-games = Play a game to get started
search-games-found = 
    { $count -> 
        [zero] No games found
        [one] 1 game found
       *[other] { $count } games found
    }

populating-database = Populating database...
    This may take several minutes.
    Go grab a coffee!
populating-games = Populating games... ({ $directory })

menu-set-as-favorite = Set as Favorite
menu-unset-as-favorite = Remove from Favorites
menu-launch = Launch
menu-launch-with-core = Launch with < { $core } >
menu-reset = Reset
menu-remove-from-recents = Remove from Recents
menu-repopulate-database = Repopulate Database

settings-wifi = Wi-Fi
settings-wifi-wifi-enabled = Wi-Fi Enabled
settings-wifi-ip-address = IP Address
settings-wifi-wifi-network = Wi-Fi Network Name
settings-wifi-wifi-password = Wi-Fi Password
settings-wifi-ntp-enabled = NTP Enabled
settings-wifi-web-file-explorer = Web File Explorer
settings-wifi-telnet-enabled = Telnet Enabled
settings-wifi-ssh-enabled = SSH Enabled
settings-wifi-ftp-enabled = FTP Enabled
settings-wifi-scraper = Scraper
settings-wifi-syncthing = Syncthing Enabled
# The status row while there is no address. Progress for the first thirty seconds, then the
# likeliest cause, judged by how far the attempt got. The radio is 2.4 GHz only.
settings-wifi-searching = Searching...
settings-wifi-connecting = Connecting...
settings-wifi-getting-ip = Getting IP address...
settings-wifi-network-not-found = Network not found
settings-wifi-wrong-password = Wrong password?
settings-wifi-no-ip = No IP address

settings-clock = Date & Time
settings-clock-datetime = Date & Time
settings-clock-timezone = Timezone

settings-display = Display
settings-display-luminance = Luminance
settings-display-hue = Hue
settings-display-contrast = Contrast
settings-display-saturation = Saturation
settings-display-red = Red
settings-display-green = Green
settings-display-blue = Blue
settings-display-screen-resolution = Screen Resolution
settings-display-profile = Profile
settings-display-profile-name = Profile Name
settings-display-warmth = Warmth
settings-display-dimness = Dimness

settings-theme = Theme
settings-theme-theme = Theme
settings-theme-wallpaper = Wallpaper
settings-theme-restore-defaults = Restore Defaults
settings-theme-dark-mode = Dark Mode
settings-theme-show-battery-level = Battery Percentage
settings-theme-show-clock = Clock
settings-theme-show-wifi = Wi-Fi Icon
settings-theme-use-recents-carousel = Recents Carousel
settings-theme-boxart-width = Boxart Width
settings-theme-boxart-underlay = Boxart Underlay
settings-theme-boxart-border-radius = Boxart Border Radius
settings-theme-ui-font = UI Font
settings-theme-ui-font-size = UI Font Size
settings-theme-guide-font = Guide Font
settings-theme-guide-font-size = Guide Font Size
settings-theme-tab-font-size = Tab Font Size
settings-theme-status-bar-font-size = Status Bar Font Size
settings-theme-button-hint-font-size = Button Hint Font Size
settings-theme-button-size = Button Size
settings-theme-button-text-font-size = Button Text Font Size
settings-theme-highlight-color = Highlight Color
settings-theme-highlight-text-color = Highlight Text Color
settings-theme-foreground-color = Foreground Color
settings-theme-background-color = Background Color
settings-theme-disabled-color = Disabled Color
settings-theme-tab-color = Tab Color
settings-theme-tab-selected-color = Tab Selected Color
settings-theme-tabs-backdrop-color = Tabs Backdrop Color
settings-theme-mirror-tabs-and-status = Mirror Tabs And Status
settings-theme-button-a-color = Button A Color
settings-theme-button-b-color = Button B Color
settings-theme-button-x-color = Button X Color
settings-theme-button-y-color = Button Y Color
settings-theme-button-text-color = Button Text Color
settings-theme-button-hint-text-color = Button Hint Text Color
settings-theme-stroke-color = Text Stroke Color
settings-theme-highlight-text-stroke-color = Highlight Text Stroke Color
settings-theme-tab-stroke-color = Tab Stroke Color
settings-theme-tab-selected-stroke-color = Tab Selected Stroke Color
settings-theme-status-bar-color = Status Bar Color
settings-theme-status-backdrop-color = Status Backdrop Color
settings-theme-hide-status-in-launcher = Hide Status In Launcher
settings-theme-status-bar-stroke-color = Status Bar Stroke Color
settings-theme-stroke-width = Stroke Width
settings-theme-margin-x = Horizontal Margin
settings-theme-margin-y = Vertical Margin
settings-theme-list-margin = List Margin
settings-theme-padding-x = Horizontal Padding
settings-theme-padding-y = Vertical Padding

settings-language = Language
settings-language-language = Language

settings-about = About

settings-power = Power
settings-power-power-button-action = Power Button Action
settings-power-power-button-action-suspend = Suspend
settings-power-power-button-action-shutdown = Shutdown
settings-power-power-button-action-nothing = Nothing
settings-power-lid-close-action = Lid Close Action
settings-power-auto-sleep-when-charging = Auto Sleep When Charging
settings-power-auto-sleep-duration-minutes = Auto Sleep Duration (Minutes)
settings-power-auto-sleep-duration-disabled = Disabled
settings-power-shutdown-after = Shut Down After Suspend (Minutes)
settings-power-shutdown-after-never = Never
settings-power-charging-boot-action = When Plugged In
settings-power-charging-boot-action-charge-screen = Charge Screen
settings-power-charging-boot-action-charge-silently = Charge Silently
settings-power-charging-boot-action-power-off = Power Off
settings-power-volume-on-startup = Volume on Startup
settings-power-volume-on-startup-restore = Restore
settings-power-volume-on-startup-muted = Muted
settings-power-performance-mode = Performance Mode
settings-power-performance-mode-system = System
settings-power-performance-mode-powersave = Powersave
settings-power-performance-mode-low = Low
settings-power-performance-mode-medium = Medium
settings-power-performance-mode-high = High
settings-power-performance-mode-max = Max
# Wraps a preset that caps the clock. Kept short: this sits beside the row title.
settings-power-performance-mode-capped = { $name } ({ $mhz }MHz)

settings-power-desc-performance-system = Leaves the CPU alone. Set a mode per game in the in-game menu.
settings-power-desc-performance-powersave = Locks the CPU to its slowest speed. Too slow for GBA.
settings-power-desc-performance-low = Speeds up only when needed, to at most { $mhz }MHz.
settings-power-desc-performance-low-unknown = Speeds up only when needed. No speed cap on this device.
settings-power-desc-performance-medium = Speeds up only when needed, to at most { $mhz }MHz.
settings-power-desc-performance-medium-unknown = Speeds up only when needed. No speed cap on this device.
settings-power-desc-performance-high = Speeds up to full when a game needs it. Usual choice.
settings-power-desc-performance-max = Holds the CPU at full speed. Uses the most battery.
settings-power-desc-auto-sleep-when-charging-on = Sleeps on the idle timer even while plugged in.
settings-power-desc-auto-sleep-when-charging-off = Stays awake while plugged in.
settings-power-desc-auto-sleep-duration = Powers off after this long with no input.
settings-power-desc-auto-sleep-duration-disabled = Never powers off on its own.
settings-power-desc-charging-boot-charge-screen = Plugging in wakes the screen, shows charging, then sleeps.
settings-power-desc-charging-boot-charge-silently = Plugging in charges without ever lighting the screen.
settings-power-desc-charging-boot-power-off = Plugging in leaves the device off. Press Power to turn it on.
settings-power-desc-volume-on-startup-restore = Starts at the volume you last set.
settings-power-desc-volume-on-startup-muted = Always starts muted.
settings-power-desc-shutdown-after = Suspending powers off after { $minutes } minutes, saving your game.
settings-power-desc-shutdown-after-never = Suspending never powers off. Watch the battery.
settings-power-desc-action-suspend = Blanks the screen, then powers off after { $minutes } minutes.
settings-power-desc-action-suspend-never = Blanks the screen. Press Power to come back.
settings-power-desc-action-shutdown = Powers off, saving your place in the game.
settings-power-desc-action-nothing = Ignored. Hold Power to force a shutdown.

settings-files = Files

settings-system-update-menu = System Update
settings-system-allium-version = Allium Version
settings-system-latest-version = Latest Version
settings-system-model-name = Model Name
settings-system-firmware-version = Firmware Version
settings-system-operating-system-version = OS Version
settings-system-kernel-version = Kernel Version
settings-system-memory-used = Memory Used
settings-system-update-channel = Update Channel
settings-system-update-channel-off = Off
settings-system-update-channel-on = On
settings-system-update = System Update
settings-system-update-check = Check for Updates
settings-system-update-checking = Checking...
settings-system-update-available = Download Update
settings-system-update-downloading = Downloading...
settings-system-update-restart-to-update = Restart to Update
settings-system-update-installing = Installing...
settings-system-update-up-to-date = Up to Date
settings-system-update-restart-required = Restart the device to install the update.
settings-system-unknown-value = Unknown

settings-needs-restart-for-effect =
    You need to restart the device
    for changes to take effect.

# Menu
ingame-menu-continue = Continue
ingame-menu-save = Save
ingame-menu-load = Load
ingame-menu-reset = Reset
ingame-menu-settings = Emulator
ingame-menu-guide = Guide
ingame-menu-quit = Quit
ingame-menu-slot = Slot { $slot }
ingame-menu-slot-auto = Auto
ingame-menu-disk = Disk { $disk }
ingame-menu-performance = Speed
# Values for the per-game speed row. Short on purpose: the save-state thumbnail leaves this
# list 271px wide, so the label is capped at 164px and a long value overlaps it. The full
# names, with clock speeds, are on Settings > Power.
ingame-menu-performance-default = Default
ingame-menu-performance-powersave = Eco
ingame-menu-performance-low = Low
ingame-menu-performance-medium = Med
ingame-menu-performance-high = High
ingame-menu-performance-max = Max

guide-button-search = Search
guide-button-next = Next
guide-button-prev = Prev

# Common
button-back = Back
button-confirm = Confirm
button-edit = Edit
button-select = Select
button-launch = Launch
button-resume = Resume
button-restart = Restart
button-options = Options
button-sort = Sort
button-edit-search = Edit Search
button-restore-defaults = Restore Defaults

keyboard-button-backspace = Backspace
keyboard-button-shift = Shift

powering-off = Powering off...
charging = Charging...
