import CoreBluetooth
import Foundation

/// Generic macOS GATT peripheral for phy6252-emu.
/// stdin:  TX <hex> | QUIT
/// stdout: READY, ADV ..., CONNECTED, DISCONNECTED, SUBSCRIBED, UNSUBSCRIBED, RX <hex>

func emit(_ line: String) {
    FileHandle.standardOutput.write(Data((line + "\n").utf8))
}

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data(("BLE \(message)\n").utf8))
    exit(2)
}

func flag(_ name: String) -> String {
    guard let index = CommandLine.arguments.firstIndex(of: name),
          index + 1 < CommandLine.arguments.count
    else {
        fail("missing \(name)")
    }
    return CommandLine.arguments[index + 1]
}

func toHex(_ data: Data) -> String {
    data.map { String(format: "%02X", $0) }.joined()
}

func parseHex(_ text: String) -> Data? {
    let compact = text.filter { !$0.isWhitespace }
    guard compact.count.isMultiple(of: 2) else {
        return nil
    }
    var bytes = [UInt8]()
    bytes.reserveCapacity(compact.count / 2)
    var chars = compact.makeIterator()
    while let hi = chars.next(), let lo = chars.next() {
        guard let value = UInt8(String([hi, lo]), radix: 16) else {
            return nil
        }
        bytes.append(value)
    }
    return Data(bytes)
}

final class Host: NSObject, CBPeripheralManagerDelegate {
    private let name: String
    private let serviceUUID: CBUUID
    private let rxUUID: CBUUID
    private let txUUID: CBUUID
    private var manager: CBPeripheralManager!
    private var rxChar: CBMutableCharacteristic!
    private var txChar: CBMutableCharacteristic!
    private var service: CBMutableService!
    private var connected = false
    private var subscribers = 0
    private var pending: [Data] = []
    private var advertised = false

    init(name: String, service: String, rx: String, tx: String) {
        self.name = name
        serviceUUID = CBUUID(string: service)
        rxUUID = CBUUID(string: rx)
        txUUID = CBUUID(string: tx)
        super.init()
        manager = CBPeripheralManager(delegate: self, queue: .main)
    }

    func handleStdin(_ line: String) {
        if line == "QUIT" {
            stop()
            exit(0)
        }
        if line.hasPrefix("TX ") {
            if let payload = parseHex(String(line.dropFirst(3))) {
                send(payload)
            }
        }
    }

    func stop() {
        manager.stopAdvertising()
        manager.removeAllServices()
    }

    private func send(_ payload: Data) {
        guard subscribers > 0 else {
            return
        }
        if manager.updateValue(payload, for: txChar, onSubscribedCentrals: nil) {
            return
        }
        pending.append(payload)
    }

    private func startService() {
        rxChar = CBMutableCharacteristic(
            type: rxUUID,
            properties: [.write, .writeWithoutResponse],
            value: nil,
            permissions: [.writeable]
        )
        txChar = CBMutableCharacteristic(
            type: txUUID,
            properties: [.notify],
            value: nil,
            permissions: [.readable]
        )
        service = CBMutableService(type: serviceUUID, primary: true)
        service.characteristics = [rxChar, txChar]
        manager.add(service)
    }

    func peripheralManagerDidUpdateState(_ peripheral: CBPeripheralManager) {
        switch peripheral.state {
        case .poweredOn:
            startService()
        case .unauthorized:
            fail("Bluetooth permission denied — allow org.phy6252.blehost in System Settings > Privacy & Security > Bluetooth")
        case .unsupported:
            fail("this Mac has no BLE peripheral support")
        case .poweredOff:
            FileHandle.standardError.write(Data("BLE adapter powered off\n".utf8))
        default:
            break
        }
    }

    func peripheralManager(_ peripheral: CBPeripheralManager, didAdd service: CBService, error: Error?) {
        if let error {
            fail("add GATT service: \(error.localizedDescription)")
        }
        peripheral.startAdvertising([
            CBAdvertisementDataLocalNameKey: name,
            CBAdvertisementDataServiceUUIDsKey: [serviceUUID],
        ])
    }

    func peripheralManagerDidStartAdvertising(_ peripheral: CBPeripheralManager, error: Error?) {
        if let error {
            fail("start advertising: \(error.localizedDescription)")
        }
        if advertised {
            return
        }
        advertised = true
        emit("READY")
        emit("ADV name=\(name) service=\(serviceUUID.uuidString)")
    }

    func peripheralManager(_ peripheral: CBPeripheralManager, didReceiveWrite requests: [CBATTRequest]) {
        for request in requests {
            guard let data = request.value, request.offset == 0 else {
                peripheral.respond(to: request, withResult: .invalidOffset)
                continue
            }
            if !connected {
                connected = true
                emit("CONNECTED")
            }
            emit("RX \(toHex(data))")
            peripheral.respond(to: request, withResult: .success)
        }
    }

    func peripheralManager(
        _ peripheral: CBPeripheralManager,
        central: CBCentral,
        didSubscribeTo characteristic: CBCharacteristic
    ) {
        _ = (peripheral, central, characteristic)
        subscribers += 1
        if !connected {
            connected = true
            emit("CONNECTED")
        }
        emit("SUBSCRIBED")
    }

    func peripheralManager(
        _ peripheral: CBPeripheralManager,
        central: CBCentral,
        didUnsubscribeFrom characteristic: CBCharacteristic
    ) {
        _ = (peripheral, central, characteristic)
        if subscribers > 0 {
            subscribers -= 1
        }
        emit("UNSUBSCRIBED")
        if subscribers == 0 {
            connected = false
            emit("DISCONNECTED")
        }
    }

    func peripheralManagerIsReady(toUpdateSubscribers peripheral: CBPeripheralManager) {
        _ = peripheral
        while !pending.isEmpty {
            let payload = pending.removeFirst()
            if !manager.updateValue(payload, for: txChar, onSubscribedCentrals: nil) {
                pending.insert(payload, at: 0)
                break
            }
        }
    }
}

let host = Host(
    name: flag("--name"),
    service: flag("--service"),
    rx: flag("--rx"),
    tx: flag("--tx")
)

DispatchQueue.global(qos: .userInitiated).async {
    while let line = readLine() {
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        DispatchQueue.main.async {
            host.handleStdin(trimmed)
        }
    }
    DispatchQueue.main.async {
        host.stop()
        exit(0)
    }
}

signal(SIGTERM) { _ in
    exit(0)
}

RunLoop.main.run()
