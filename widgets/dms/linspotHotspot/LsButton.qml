import QtQuick
import qs.Common
import qs.Widgets

// Tiny self-contained button so the plugin depends on as few DMS widgets as possible.
Rectangle {
    id: btn
    property string text: ""
    property string iconName: ""
    property bool primary: false
    property bool active: true
    signal clicked()

    implicitWidth: row.implicitWidth + Theme.spacingM * 2
    implicitHeight: 36
    radius: Theme.cornerRadius
    color: primary ? Theme.primary : Theme.chipSurface
    opacity: active ? (area.containsMouse ? 0.85 : 1.0) : 0.45

    Row {
        id: row
        anchors.centerIn: parent
        spacing: 6
        DankIcon {
            visible: btn.iconName !== ""
            name: btn.iconName
            size: 18
            color: btn.primary ? Theme.primaryText : Theme.surfaceText
            anchors.verticalCenter: parent.verticalCenter
        }
        StyledText {
            visible: btn.text !== ""
            text: btn.text
            font.pixelSize: Theme.fontSizeMedium
            color: btn.primary ? Theme.primaryText : Theme.surfaceText
            anchors.verticalCenter: parent.verticalCenter
        }
    }

    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        enabled: btn.active
        cursorShape: Qt.PointingHandCursor
        onClicked: btn.clicked()
    }
}
