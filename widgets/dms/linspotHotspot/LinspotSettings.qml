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
        text: "SSID, password, band and channel come from the LinSpot app (~/.config/linspot/config.json). Open the app once (right-click the bar icon) and save your settings. Left-click the bar icon for the manage popout; right-click toggles the hotspot."
        font.pixelSize: Theme.fontSizeSmall
        color: Theme.surfaceVariantText
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
