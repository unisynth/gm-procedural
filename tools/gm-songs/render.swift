// gm-songs renderer: plays a pre-timed MIDI event list through Apple's AUMIDISynth with a given DLS bank,
// offline, and writes a 32-bit float stereo WAV. The same renderer plays both banks, so differences come
// from the bank only.
//
// usage: gmrender <bank.dls> <events.txt> <out.wav> [rate]
// events.txt: one event per line, "frame b0 b1 b2" (decimal, b2 may be absent), sorted by frame.
// The first line "len <frames>" gives the length to render.
import AVFoundation
import AudioToolbox

func die(_ m: String) -> Never { FileHandle.standardError.write((m + "\n").data(using: .utf8)!); exit(1) }

let a = CommandLine.arguments
if a.count < 4 { die("usage: gmrender <bank.dls> <events.txt> <out.wav> [rate]") }
let rate = a.count > 4 ? Double(a[4])! : 44100
let text = (try? String(contentsOfFile: a[2], encoding: .utf8)) ?? { die("cannot read \(a[2])") }()
var total = 0
var events: [(Int, [UInt8])] = []
for line in text.split(separator: "\n") {
    let f = line.split(separator: " ")
    if f.first == "len" { total = Int(f[1])!; continue }
    events.append((Int(f[0])!, f.dropFirst().map { UInt8($0)! }))
}

let desc = AudioComponentDescription(componentType: kAudioUnitType_MusicDevice, componentSubType: kAudioUnitSubType_MIDISynth,
                                     componentManufacturer: kAudioUnitManufacturer_Apple, componentFlags: 0, componentFlagsMask: 0)
let engine = AVAudioEngine()
let synth = AVAudioUnitMIDIInstrument(audioComponentDescription: desc)
engine.attach(synth)
let fmt = AVAudioFormat(standardFormatWithSampleRate: rate, channels: 2)!
engine.connect(synth, to: engine.mainMixerNode, format: fmt)
let au = synth.audioUnit
var url: Unmanaged<CFURL> = Unmanaged.passRetained(URL(fileURLWithPath: a[1]) as CFURL)
var st = AudioUnitSetProperty(au, kMusicDeviceProperty_SoundBankURL, kAudioUnitScope_Global, 0, &url, UInt32(MemoryLayout<CFURL>.size))
if st != noErr { die("bank load failed: \(st)") }
do { try engine.enableManualRenderingMode(.offline, format: fmt, maximumFrameCount: 512); try engine.start() } catch { die("engine: \(error)") }

// Preload every (channel, bank, program) the song uses, so no instrument loads lazily mid-render.
func send(_ b: [UInt8], _ off: UInt32 = 0) {
    MusicDeviceMIDIEvent(au, UInt32(b[0]), UInt32(b.count > 1 ? b[1] : 0), UInt32(b.count > 2 ? b[2] : 0), off)
}
var on: UInt32 = 1
AudioUnitSetProperty(au, kAUMIDISynthProperty_EnablePreload, kAudioUnitScope_Global, 0, &on, 4)
var bankMsb = [UInt8](repeating: 0, count: 16)
for (_, b) in events {
    let ch = Int(b[0] & 15)
    if b[0] & 0xF0 == 0xB0 && b[1] == 0 { bankMsb[ch] = b[2] }
    if b[0] & 0xF0 == 0xC0 { send([0xB0 | UInt8(ch), 0, bankMsb[ch]]); send(b) }
}
for ch in 0..<16 { send([0xC0 | UInt8(ch), 0]) }
var off: UInt32 = 0
AudioUnitSetProperty(au, kAUMIDISynthProperty_EnablePreload, kAudioUnitScope_Global, 0, &off, 4)
for ch in 0..<16 { send([0xB0 | UInt8(ch), 0, 0]); send([0xC0 | UInt8(ch), 0]) }

// In a function so the AVAudioFile is released (and its header finalised) before exit.
func renderAll() {
let buf = AVAudioPCMBuffer(pcmFormat: fmt, frameCapacity: 512)!
let file: AVAudioFile
do {
    let settings: [String: Any] = [AVFormatIDKey: kAudioFormatLinearPCM, AVSampleRateKey: rate, AVNumberOfChannelsKey: 2,
                                   AVLinearPCMBitDepthKey: 32, AVLinearPCMIsFloatKey: true, AVLinearPCMIsNonInterleaved: false]
    file = try AVAudioFile(forWriting: URL(fileURLWithPath: a[3]), settings: settings, commonFormat: .pcmFormatFloat32, interleaved: false)
} catch { die("out: \(error)") }
var pos = 0, i = 0
while pos < total {
    let n = min(512, total - pos)
    while i < events.count && events[i].0 < pos + n {
        send(events[i].1, UInt32(max(0, events[i].0 - pos)))
        i += 1
    }
    do {
        let s = try engine.renderOffline(AVAudioFrameCount(n), to: buf)
        if s != .success { die("render status \(s.rawValue)") }
        try file.write(from: buf)
    } catch { die("render: \(error)") }
    pos += n
}
}
renderAll()
