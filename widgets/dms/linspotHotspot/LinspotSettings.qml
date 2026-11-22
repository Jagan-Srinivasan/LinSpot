import QtQuick
import qs.Common
import qs.Widgets
import qs.Modules.Plugins

PluginSettings {
    pluginId: "linspotHotspot"

    StyledText {
        width: parent.width
        text: "LinSpot Hotspot"
        font.pixelSize: Theme.fontSizeLarge
        font.weight: Font.Bold
        color: Theme.surfaceText
    }

    StyledText {
        width: parent.width
        wrapMode: Text.WordWrap
        text: "SSID, password, band and channel come from the LinSpot app (~/.config/linspot/config.json). Left-click the bar icon for the manage popout; right-click toggles the hotspot."
        font.pixelSize: Theme.fontSizeSmall
        color: Theme.surfaceVariantText
    }

    // ---------------------------------------------------------- authentication
    StyledText {
        width: parent.width
        text: "Authentication"
        font.pixelSize: Theme.fontSizeMedium
        font.weight: Font.Medium
        color: Theme.surfaceText
    }

    SelectionSetting {
        settingKey: "authMode"
        label: "Password prompt"
        description: "Inline: ask for your sudo password inside the control center / popout (nothing else opens, so it can't close). Polkit: use the pkexec dialog."
        options: [
            { label: "Inline (sudo)", value: "sudo" },
            { label: "Polkit dialog (pkexec)", value: "pkexec" }
        ]
        defaultValue: "sudo"
    }

    ToggleSetting {
        settingKey: "fingerprintOffer"
        label: "Allow fingerprint"
        description: "Adds a fingerprint button next to the password box. Needs enrolled fingers (fprintd-enroll) and pam_fprintd in your sudo PAM stack."
        defaultValue: true
    }

    ToggleSetting {
        settingKey: "fingerprintAuto"
        label: "Scan fingerprint automatically"
        description: "Start listening to the sensor as soon as authentication is needed; the password box stays available as a fallback"
        defaultValue: true
    }

    // ------------------------------------------------------------- bar pill
    StyledText {
        width: parent.width
        text: "Bar widget"
        font.pixelSize: Theme.fontSizeMedium
        font.weight: Font.Medium
        color: Theme.surfaceText
    }

    ToggleSetting {
        settingKey: "hideBarWhenOff"
        label: "Hide bar widget when hotspot is off"
        description: "The bar icon only appears while the hotspot is on. The control center tile is always available."
        defaultValue: true
    }

    ToggleSetting {
        settingKey: "showCount"
        label: "Show connected device count"
        description: "Number next to the icon while the hotspot is on"
        defaultValue: true
    }

    ToggleSetting {
        settingKey: "showZero"
        label: "Show count when 0 devices"
        description: "Off hides the number until someone connects"
        defaultValue: true
    }

    SliderSetting {
        settingKey: "pillIconSize"
        label: "Icon size"
        description: "Pixels"
        defaultValue: 18
        minimum: 12
        maximum: 32
        unit: "px"
    }

    SliderSetting {
        settingKey: "pillFontSize"
        label: "Count text size"
        description: "Pixels"
        defaultValue: 12
        minimum: 8
        maximum: 20
        unit: "px"
    }

    SliderSetting {
        settingKey: "pillPadding"
        label: "Side padding"
        description: "Space between the pill edge and its content"
        defaultValue: 8
        minimum: 0
        maximum: 24
        unit: "px"
    }

    SliderSetting {
        settingKey: "pillSpacing"
        label: "Icon / count spacing"
        description: "Gap between the icon and the number"
        defaultValue: 4
        minimum: 0
        maximum: 16
        unit: "px"
    }

    // ---------------------------------------------------------------- popout
    StyledText {
        width: parent.width
        text: "Popout / control center panel"
        font.pixelSize: Theme.fontSizeMedium
        font.weight: Font.Medium
        color: Theme.surfaceText
    }

    SliderSetting {
        settingKey: "popoutWidth"
        label: "Popout width"
        description: "Bar popout only"
        defaultValue: 380
        minimum: 300
        maximum: 600
        unit: "px"
    }

    SliderSetting {
        settingKey: "popoutHeight"
        label: "Popout height"
        description: "Bar popout only"
        defaultValue: 420
        minimum: 240
        maximum: 800
        unit: "px"
    }

    SliderSetting {
        settingKey: "maxVisibleDevices"
        label: "Visible devices before scrolling"
        description: "The device list scrolls instead of growing past this"
        defaultValue: 4
        minimum: 1
        maximum: 10
        unit: ""
    }

    // -------------------------------------------------------------- advanced
    StyledText {
        width: parent.width
        text: "Advanced"
        font.pixelSize: Theme.fontSizeMedium
        font.weight: Font.Medium
        color: Theme.surfaceText
    }

    StringSetting {
        settingKey: "linspotBin"
        label: "LinSpot binary (optional)"
        description: "Leave empty to auto-detect from /etc/linspot/install.info"
        placeholder: "/usr/local/bin/linspot"
        defaultValue: ""
    }

    StringSetting {
        settingKey: "configPath"
        label: "Config file (optional)"
        description: "Leave empty for ~/.config/linspot/config.json"
        placeholder: ""
        defaultValue: ""
    }

    StringSetting {
        settingKey: "pollSeconds"
        label: "Refresh interval (seconds)"
        description: "How often status and connected devices are polled"
        placeholder: "3"
        defaultValue: "3"
    }
}
