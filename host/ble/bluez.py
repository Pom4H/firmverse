#!/usr/bin/env python3
"""Generic Linux BlueZ GATT peripheral for phy6252-emu.

stdin:  TX <hex> | QUIT
stdout: READY, ADV ..., CONNECTED, DISCONNECTED, SUBSCRIBED, UNSUBSCRIBED, RX <hex>
"""

import argparse
import signal
import sys

try:
    import dbus
    import dbus.mainloop.glib
    import dbus.service
    from gi.repository import GLib
except ImportError as exc:
    sys.stderr.write(
        "BlueZ helper needs python3-dbus and python3-gi (plus bluez/bluetoothd): %s\n" % exc
    )
    raise SystemExit(2)

BLUEZ = "org.bluez"
OM = "org.freedesktop.DBus.ObjectManager"
PROPS = "org.freedesktop.DBus.Properties"
ADAPTER = "org.bluez.Adapter1"
GATT_MANAGER = "org.bluez.GattManager1"
ADV_MANAGER = "org.bluez.LEAdvertisingManager1"
GATT_SERVICE = "org.bluez.GattService1"
GATT_CHRC = "org.bluez.GattCharacteristic1"
ADV_IFACE = "org.bluez.LEAdvertisement1"


def emit(line):
    sys.stdout.write(line + "\n")
    sys.stdout.flush()


def to_hex(value):
    return bytes(value).hex().upper()


def parse_hex(text):
    try:
        return bytes.fromhex(text.strip())
    except ValueError:
        return None


class Application(dbus.service.Object):
    def __init__(self, bus, service):
        super().__init__(bus, "/")
        self.service = service

    @dbus.service.method(OM, out_signature="a{oa{sa{sv}}}")
    def GetManagedObjects(self):
        objects = {self.service.path: self.service.props()}
        for ch in self.service.characteristics:
            objects[ch.path] = ch.props()
        return objects


class Service(dbus.service.Object):
    def __init__(self, bus, uuid):
        self.path = "/org/phy6252/service0"
        self.uuid = uuid
        self.characteristics = []
        super().__init__(bus, self.path)

    def add(self, ch):
        self.characteristics.append(ch)

    def props(self):
        return {
            GATT_SERVICE: {
                "UUID": self.uuid,
                "Primary": dbus.Boolean(True),
                "Characteristics": dbus.Array(
                    [dbus.ObjectPath(ch.path) for ch in self.characteristics], signature="o"
                ),
            }
        }

    @dbus.service.method(PROPS, in_signature="s", out_signature="a{sv}")
    def GetAll(self, interface):
        if interface != GATT_SERVICE:
            raise dbus.exceptions.DBusException("org.bluez.Error.InvalidArguments")
        return self.props()[GATT_SERVICE]


class Characteristic(dbus.service.Object):
    def __init__(self, bus, service, index, uuid, flags):
        self.service = service
        self.path = "%s/char%d" % (service.path, index)
        self.uuid = uuid
        self.flags = flags
        super().__init__(bus, self.path)

    def props(self):
        return {
            GATT_CHRC: {
                "Service": dbus.ObjectPath(self.service.path),
                "UUID": self.uuid,
                "Flags": dbus.Array(self.flags, signature="s"),
                "Descriptors": dbus.Array([], signature="o"),
            }
        }

    @dbus.service.method(PROPS, in_signature="s", out_signature="a{sv}")
    def GetAll(self, interface):
        if interface != GATT_CHRC:
            raise dbus.exceptions.DBusException("org.bluez.Error.InvalidArguments")
        return self.props()[GATT_CHRC]

    @dbus.service.method(GATT_CHRC, in_signature="a{sv}", out_signature="ay")
    def ReadValue(self, _options):
        raise dbus.exceptions.DBusException("org.bluez.Error.NotSupported")

    @dbus.service.method(GATT_CHRC, in_signature="aya{sv}")
    def WriteValue(self, _value, _options):
        raise dbus.exceptions.DBusException("org.bluez.Error.NotSupported")

    @dbus.service.method(GATT_CHRC)
    def StartNotify(self):
        raise dbus.exceptions.DBusException("org.bluez.Error.NotSupported")

    @dbus.service.method(GATT_CHRC)
    def StopNotify(self):
        raise dbus.exceptions.DBusException("org.bluez.Error.NotSupported")

    @dbus.service.signal(PROPS, signature="sa{sv}as")
    def PropertiesChanged(self, _interface, _changed, _invalidated):
        pass


class RxCharacteristic(Characteristic):
    def __init__(self, bus, service, uuid):
        super().__init__(bus, service, 0, uuid, ["write", "write-without-response"])
        self.connected = False

    @dbus.service.method(GATT_CHRC, in_signature="aya{sv}")
    def WriteValue(self, value, _options):
        if not self.connected:
            self.connected = True
            emit("CONNECTED")
        emit("RX " + to_hex(value))


class TxCharacteristic(Characteristic):
    def __init__(self, bus, service, uuid):
        super().__init__(bus, service, 1, uuid, ["notify"])
        self.notifying = False

    @dbus.service.method(GATT_CHRC)
    def StartNotify(self):
        if self.notifying:
            return
        self.notifying = True
        emit("CONNECTED")
        emit("SUBSCRIBED")

    @dbus.service.method(GATT_CHRC)
    def StopNotify(self):
        if not self.notifying:
            return
        self.notifying = False
        emit("UNSUBSCRIBED")
        emit("DISCONNECTED")

    def send(self, payload):
        if not self.notifying:
            return
        value = dbus.Array([dbus.Byte(b) for b in payload], signature="y")
        self.PropertiesChanged(GATT_CHRC, {"Value": value}, [])


class Advertisement(dbus.service.Object):
    def __init__(self, bus, name, service_uuid):
        self.path = "/org/phy6252/advertisement0"
        self.name = name
        self.service_uuid = service_uuid
        super().__init__(bus, self.path)

    def props(self):
        return {
            "Type": "peripheral",
            "ServiceUUIDs": dbus.Array([self.service_uuid], signature="s"),
            "LocalName": self.name,
            "Discoverable": dbus.Boolean(True),
        }

    @dbus.service.method(PROPS, in_signature="s", out_signature="a{sv}")
    def GetAll(self, interface):
        if interface != ADV_IFACE:
            raise dbus.exceptions.DBusException("org.bluez.Error.InvalidArguments")
        return self.props()

    @dbus.service.method(ADV_IFACE)
    def Release(self):
        emit("RELEASED")


def find_adapter(bus):
    root = dbus.Interface(bus.get_object(BLUEZ, "/"), OM)
    for path, interfaces in root.GetManagedObjects().items():
        if GATT_MANAGER in interfaces and ADV_MANAGER in interfaces:
            return str(path)
    return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--name", default="PB03FKIT")
    parser.add_argument("--service", required=True)
    parser.add_argument("--rx", required=True)
    parser.add_argument("--tx", required=True)
    args = parser.parse_args()

    dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
    bus = dbus.SystemBus()
    adapter = find_adapter(bus)
    if not adapter:
        raise SystemExit("no BlueZ adapter exposing GattManager1 + LEAdvertisingManager1")

    adapter_obj = bus.get_object(BLUEZ, adapter)
    adapter_props = dbus.Interface(adapter_obj, PROPS)
    adapter_props.Set(ADAPTER, "Powered", dbus.Boolean(True))

    service = Service(bus, args.service)
    rx = RxCharacteristic(bus, service, args.rx)
    tx = TxCharacteristic(bus, service, args.tx)
    service.add(rx)
    service.add(tx)
    app = Application(bus, service)
    adv = Advertisement(bus, args.name, args.service)

    gatt = dbus.Interface(adapter_obj, GATT_MANAGER)
    adv_mgr = dbus.Interface(adapter_obj, ADV_MANAGER)
    gatt.RegisterApplication(dbus.ObjectPath("/"), {})
    adv_mgr.RegisterAdvertisement(dbus.ObjectPath(adv.path), {})

    emit("READY")
    emit("ADV name=%s service=%s" % (args.name, args.service))

    loop = GLib.MainLoop()

    def stdin_ready(_fd, _condition):
        line = sys.stdin.readline()
        if line == "":
            loop.quit()
            return False
        line = line.strip()
        if line == "QUIT":
            loop.quit()
            return False
        if line.startswith("TX "):
            payload = parse_hex(line[3:])
            if payload is not None:
                tx.send(payload)
        return True

    GLib.io_add_watch(sys.stdin, GLib.IO_IN | GLib.IO_HUP, stdin_ready)
    signal.signal(signal.SIGTERM, lambda _s, _f: loop.quit())
    signal.signal(signal.SIGINT, lambda _s, _f: loop.quit())

    try:
        loop.run()
    finally:
        try:
            adv_mgr.UnregisterAdvertisement(dbus.ObjectPath(adv.path))
        except dbus.DBusException:
            pass
        try:
            gatt.UnregisterApplication(dbus.ObjectPath("/"))
        except dbus.DBusException:
            pass


if __name__ == "__main__":
    main()
