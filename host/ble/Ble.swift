import CoreBluetooth
import Foundation

/// PB-03F-Kit demo GATT (not a vendor stack). RX = writes from the phone, TX = notifies from the hex.
let serviceUUID = CBUUID(string: "6B1D0001-7C8E-4A91-9F2B-E3A14C5B0001")
let rxUUID = CBUUID(string: "6B1D0002-7C8E-4A91-9F2B-E3A14C5B0001")
let txUUID = CBUUID(string: "6B1D0003-7C8E-4A91-9F2B-E3A14C5B0001")

func emit(_ line: String) {
    fputs(line + "\n", stdout)
    fflush(stdout)
}

func parseHex(_ text: String) -> Data? {
    let clean = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard clean.count % 2 == 0, clean.count > 0 else { return nil }
    var data = Data()
    var index = clean.startIndex
    while index < clean.endIndex {
        let next = clean.index(index, offsetBy: 2)
        guard let byte = UInt8(clean[index..<next], radix: 16) else { return nil }
        data.append(byte)
        index = next
    }
    return data
}

func hex(_ data: Data) -> String {
    data.map { String(format: "%02X", $0) }.joined()
}

final class LineReader {
    private var buffer = Data()
    var onLine: ((String) -> Void)?

    func feed(_ chunk: Data) {
        buffer.append(chunk)
        while let range = buffer.firstRange(of: Data([0x0A])) {
            let lineData = buffer.subdata(in: buffer.startIndex..<range.lowerBound)
            buffer.removeSubrange(buffer.startIndex..<range.upperBound)
            var line = String(data: lineData, encoding: .utf8) ?? ""
            if line.hasSuffix("\r") { line.removeLast() }
            if !line.isEmpty { onLine?(line) }
        }
    }
}

final class Peripheral: NSObject, CBPeripheralManagerDelegate {
    private var manager: CBPeripheralManager!
    private var rxChar: CBMutableCharacteristic!
    private var txChar: CBMutableCharacteristic!
    private var pendingTx: [Data] = []
    private let name: String

    init(name: String) {
        self.name = name
    }

    func start() {
        manager = CBPeripheralManager(delegate: self, queue: DispatchQueue.main)
    }

    func handle(_ line: String) {
        if line == "QUIT" {
            manager.stopAdvertising()
            exit(0)
        }
        if line.hasPrefix("TX ") {
            let hexStr = String(line.dropFirst(3)).trimmingCharacters(in: .whitespaces)
            guard let data = parseHex(hexStr) else { return }
            if txChar == nil { return }
            if !manager.updateValue(data, for: txChar, onSubscribedCentrals: nil) {
                pendingTx.append(data)
            }
        }
    }

    func peripheralManagerDidUpdateState(_ peripheral: CBPeripheralManager) {
        guard peripheral.state == .poweredOn else {
            emit("ERROR bluetooth \(peripheral.state.rawValue)")
            return
        }
        rxChar = CBMutableCharacteristic(
            type: rxUUID,
            properties: [.write, .writeWithoutResponse],
            value: nil,
            permissions: [.writeable]
        )
        txChar = CBMutableCharacteristic(
            type: txUUID,
            properties: [.notify, .indicate],
            value: nil,
            permissions: [.readable]
        )
        let service = CBMutableService(type: serviceUUID, primary: true)
        service.characteristics = [rxChar, txChar]
        peripheral.removeAllServices()
        peripheral.add(service)
    }

    func peripheralManager(_ peripheral: CBPeripheralManager, didAdd service: CBService, error: Error?) {
        if let error {
            emit("ERROR \(error.localizedDescription)")
            return
        }
        // Flags + 128-bit UUID leave ~8 bytes for the local name in the ADV PDU.
        peripheral.startAdvertising([
            CBAdvertisementDataLocalNameKey: name,
            CBAdvertisementDataServiceUUIDsKey: [serviceUUID],
        ])
        emit("READY")
        emit("ADV \(name)")
    }

    func peripheralManager(_ peripheral: CBPeripheralManager, didReceiveWrite requests: [CBATTRequest]) {
        for request in requests {
            if request.characteristic.uuid == rxUUID, let value = request.value {
                emit("RX \(hex(value))")
                peripheral.respond(to: request, withResult: .success)
            } else {
                peripheral.respond(to: request, withResult: .requestNotSupported)
            }
        }
    }

    func peripheralManager(
        _ peripheral: CBPeripheralManager,
        central: CBCentral,
        didSubscribeTo characteristic: CBCharacteristic
    ) {
        if characteristic.uuid == txUUID {
            emit("CONNECTED")
            emit("SUBSCRIBED")
        }
    }

    func peripheralManager(
        _ peripheral: CBPeripheralManager,
        central: CBCentral,
        didUnsubscribeFrom characteristic: CBCharacteristic
    ) {
        if characteristic.uuid == txUUID {
            emit("DISCONNECTED")
        }
    }

    func peripheralManagerIsReady(toUpdateSubscribers peripheral: CBPeripheralManager) {
        while let data = pendingTx.first {
            if peripheral.updateValue(data, for: txChar, onSubscribedCentrals: nil) {
                pendingTx.removeFirst()
            } else {
                break
            }
        }
    }
}

func argValue(_ args: [String], _ name: String) -> String? {
    guard let index = args.firstIndex(of: name), index + 1 < args.count else { return nil }
    return args[index + 1]
}

let args = Array(CommandLine.arguments.dropFirst())
let name = argValue(args, "--name") ?? "PB03FKIT"
let peripheral = Peripheral(name: String(name.prefix(8)))
let reader = LineReader()
reader.onLine = { line in DispatchQueue.main.async { peripheral.handle(line) } }
FileHandle.standardInput.readabilityHandler = { handle in
    let chunk = handle.availableData
    if chunk.isEmpty { exit(0) }
    DispatchQueue.main.async { reader.feed(chunk) }
}
peripheral.start()
RunLoop.main.run()
