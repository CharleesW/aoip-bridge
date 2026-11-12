// AoIP Bridge: driver de audio virtual para macOS (AudioServerPlugIn).
// Publica uma placa "AoIP Bridge" de 32 entradas e 32 saidas, 48 kHz, float32.
// O macOS roda este codigo num processo restrito e de prioridade baixa, entao aqui nao ha rede
// AES67 nem painel: o driver so troca audio, por UDP em 127.0.0.1, com o programa auxiliar
// `aoip-virtual` (src/main.rs). Tudo e indexado pelo relogio da placa (quadros desde que ligou):
//   WriteMix:  o que os programas tocaram, com a hora em que deve soar -> auxiliar -> rede
//   ReadInput: o que o auxiliar mandou da rede para aquela hora        -> programas
// Nenhuma thread propria: envio e recepcao acontecem dentro do ciclo de audio do sistema.
// Licenca: GPL-3.0-or-later.
#include <CoreAudio/AudioServerPlugIn.h>
#include <arpa/inet.h>
#include <fcntl.h>
#include <mach/mach_time.h>
#include <netinet/in.h>
#include <pthread.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

enum { kPlugIn = kAudioObjectPlugInObject, kDevice = 2, kStreamIn = 3, kStreamOut = 4 };
#define CHANNELS 32
#define RATE 48000.0
#define RING 16384 /* quadros entre dois "zeros" do relogio da placa */
#define HELPER_PORT 48788
#define DRIVER_PORT 48789
#define MAGIC 0x414F4950u /* "AOIP" */
#define CHUNK 32      /* quadros por pacote para o auxiliar (4 KB) */
#define MAX_FRAMES 64 /* maior pacote aceito do auxiliar */
#define MARGIN 192    /* folga de 4 ms: o auxiliar pode atrasar ate isso sem falha no som */
#define INRING 8192   /* quadros guardados do que veio da rede */
enum { kKindOut = 1, kKindBeat = 2, kKindIn = 3 };

typedef struct {
    UInt32 magic, epoch;
    UInt64 anchor;
    Float64 ticksPerFrame;
    SInt64 sampleTime;
    UInt32 frames, kind;
} Header; /* 40 bytes, sem enchimento */
typedef struct { Header h; float data[MAX_FRAMES * CHANNELS]; } Packet;

static int gSock = -1;
static struct sockaddr_in gHelper;
static UInt32 gEpoch = 0;
static float gIn[INRING * CHANNELS];
static SInt64 gInHigh = 0; /* quadros validos: abaixo deste */
static Boolean gInValid = false;

#define DEVICE_UID "AoIPBridge_UID"
#define MODEL_UID "AoIPBridge_Model"

static pthread_mutex_t gLock = PTHREAD_MUTEX_INITIALIZER;
static UInt32 gRefs = 0, gIOCount = 0;
static UInt64 gAnchor = 0, gStamps = 0;
static Float64 gTicksPerFrame = 0;
static AudioServerPlugInHostRef gHost = NULL;

// ------------------------------------------------------------------ propriedades

#define SCALAR(type, value) do { \
        *outSize = sizeof(type); \
        if (out) { if (inSize < sizeof(type)) return kAudioHardwareBadPropertySizeError; *(type*)out = (value); } \
        return 0; } while (0)

static OSStatus List(const AudioObjectID* items, UInt32 count, UInt32 inSize, UInt32* outSize, void* out)
{
    if (out) {
        UInt32 n = inSize / sizeof(AudioObjectID);
        if (n > count) n = count;
        memcpy(out, items, n * sizeof(AudioObjectID));
        *outSize = n * sizeof(AudioObjectID);
    } else {
        *outSize = count * sizeof(AudioObjectID);
    }
    return 0;
}

static AudioStreamBasicDescription Format(void)
{
    AudioStreamBasicDescription f;
    memset(&f, 0, sizeof(f));
    f.mSampleRate = RATE;
    f.mFormatID = kAudioFormatLinearPCM;
    f.mFormatFlags = kAudioFormatFlagIsFloat | kAudioFormatFlagsNativeEndian | kAudioFormatFlagIsPacked;
    f.mBytesPerPacket = 4 * CHANNELS;
    f.mFramesPerPacket = 1;
    f.mBytesPerFrame = 4 * CHANNELS;
    f.mChannelsPerFrame = CHANNELS;
    f.mBitsPerChannel = 32;
    return f;
}

// Uma funcao so para "existe?", "que tamanho?" e "qual o valor?": com `out` nulo so calcula o tamanho.
static OSStatus Prop(AudioObjectID obj, const AudioObjectPropertyAddress* a, UInt32 inSize, UInt32* outSize, void* out)
{
    const AudioObjectID both[2] = { kStreamIn, kStreamOut };
    const AudioObjectID dev[1] = { kDevice };
    switch (obj) {
    case kPlugIn:
        switch (a->mSelector) {
        case kAudioObjectPropertyBaseClass: SCALAR(AudioClassID, kAudioObjectClassID);
        case kAudioObjectPropertyClass: SCALAR(AudioClassID, kAudioPlugInClassID);
        case kAudioObjectPropertyOwner: SCALAR(AudioObjectID, kAudioObjectUnknown);
        case kAudioObjectPropertyManufacturer: SCALAR(CFStringRef, CFSTR("aoip-bridge"));
        case kAudioObjectPropertyOwnedObjects:
        case kAudioPlugInPropertyDeviceList: return List(dev, 1, inSize, outSize, out);
        case kAudioPlugInPropertyBoxList: return List(dev, 0, inSize, outSize, out);
        case kAudioPlugInPropertyTranslateUIDToDevice: SCALAR(AudioObjectID, kDevice);
        case kAudioPlugInPropertyTranslateUIDToBox: SCALAR(AudioObjectID, kAudioObjectUnknown);
        case kAudioPlugInPropertyResourceBundle: SCALAR(CFStringRef, CFSTR(""));
        }
        return kAudioHardwareUnknownPropertyError;

    case kDevice:
        switch (a->mSelector) {
        case kAudioObjectPropertyBaseClass: SCALAR(AudioClassID, kAudioObjectClassID);
        case kAudioObjectPropertyClass: SCALAR(AudioClassID, kAudioDeviceClassID);
        case kAudioObjectPropertyOwner: SCALAR(AudioObjectID, kPlugIn);
        case kAudioObjectPropertyName: SCALAR(CFStringRef, CFSTR("AoIP Bridge"));
        case kAudioObjectPropertyManufacturer: SCALAR(CFStringRef, CFSTR("aoip-bridge"));
        case kAudioDevicePropertyDeviceUID: SCALAR(CFStringRef, CFSTR(DEVICE_UID));
        case kAudioDevicePropertyModelUID: SCALAR(CFStringRef, CFSTR(MODEL_UID));
        case kAudioDevicePropertyTransportType: SCALAR(UInt32, kAudioDeviceTransportTypeVirtual);
        case kAudioDevicePropertyRelatedDevices: return List(dev, 1, inSize, outSize, out);
        case kAudioDevicePropertyClockDomain: SCALAR(UInt32, 0);
        case kAudioDevicePropertyDeviceIsAlive: SCALAR(UInt32, 1);
        case kAudioDevicePropertyDeviceIsRunning: SCALAR(UInt32, gIOCount > 0 ? 1 : 0);
        case kAudioDevicePropertyDeviceCanBeDefaultDevice: SCALAR(UInt32, 1);
        case kAudioDevicePropertyDeviceCanBeDefaultSystemDevice: SCALAR(UInt32, 1);
        case kAudioDevicePropertyLatency: SCALAR(UInt32, MARGIN);
        case kAudioDevicePropertySafetyOffset: SCALAR(UInt32, 0);
        case kAudioDevicePropertyIsHidden: SCALAR(UInt32, 0);
        case kAudioDevicePropertyZeroTimeStampPeriod: SCALAR(UInt32, RING);
        case kAudioDevicePropertyNominalSampleRate: SCALAR(Float64, RATE);
        case kAudioObjectPropertyControlList: return List(dev, 0, inSize, outSize, out);
        case kAudioObjectPropertyOwnedObjects:
        case kAudioDevicePropertyStreams:
            if (a->mScope == kAudioObjectPropertyScopeInput) return List(&both[0], 1, inSize, outSize, out);
            if (a->mScope == kAudioObjectPropertyScopeOutput) return List(&both[1], 1, inSize, outSize, out);
            return List(both, 2, inSize, outSize, out);
        case kAudioDevicePropertyAvailableNominalSampleRates:
            *outSize = sizeof(AudioValueRange);
            if (out) {
                if (inSize < sizeof(AudioValueRange)) { *outSize = 0; return 0; }
                ((AudioValueRange*)out)->mMinimum = RATE;
                ((AudioValueRange*)out)->mMaximum = RATE;
            }
            return 0;
        case kAudioDevicePropertyPreferredChannelsForStereo:
            *outSize = 2 * sizeof(UInt32);
            if (out) {
                if (inSize < 2 * sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
                ((UInt32*)out)[0] = 1;
                ((UInt32*)out)[1] = 2;
            }
            return 0;
        }
        return kAudioHardwareUnknownPropertyError;

    case kStreamIn:
    case kStreamOut:
        switch (a->mSelector) {
        case kAudioObjectPropertyBaseClass: SCALAR(AudioClassID, kAudioObjectClassID);
        case kAudioObjectPropertyClass: SCALAR(AudioClassID, kAudioStreamClassID);
        case kAudioObjectPropertyOwner: SCALAR(AudioObjectID, kDevice);
        case kAudioObjectPropertyName: SCALAR(CFStringRef, obj == kStreamIn ? CFSTR("Da rede") : CFSTR("Para a rede"));
        case kAudioStreamPropertyIsActive: SCALAR(UInt32, 1);
        case kAudioStreamPropertyDirection: SCALAR(UInt32, obj == kStreamIn ? 1 : 0);
        case kAudioStreamPropertyTerminalType:
            SCALAR(UInt32, obj == kStreamIn ? kAudioStreamTerminalTypeLine : kAudioStreamTerminalTypeLine);
        case kAudioStreamPropertyStartingChannel: SCALAR(UInt32, 1);
        case kAudioStreamPropertyLatency: SCALAR(UInt32, 0);
        case kAudioStreamPropertyVirtualFormat:
        case kAudioStreamPropertyPhysicalFormat: SCALAR(AudioStreamBasicDescription, Format());
        case kAudioStreamPropertyAvailableVirtualFormats:
        case kAudioStreamPropertyAvailablePhysicalFormats:
            *outSize = sizeof(AudioStreamRangedDescription);
            if (out) {
                if (inSize < sizeof(AudioStreamRangedDescription)) { *outSize = 0; return 0; }
                AudioStreamRangedDescription* r = (AudioStreamRangedDescription*)out;
                r->mFormat = Format();
                r->mSampleRateRange.mMinimum = RATE;
                r->mSampleRateRange.mMaximum = RATE;
            }
            return 0;
        }
        return kAudioHardwareUnknownPropertyError;
    }
    return kAudioHardwareBadObjectError;
}

static Boolean HasProperty(AudioServerPlugInDriverRef d, AudioObjectID obj, pid_t pid, const AudioObjectPropertyAddress* a)
{
    UInt32 size = 0;
    return a != NULL && Prop(obj, a, 0, &size, NULL) == 0;
}

static Boolean IsFormat(AudioObjectPropertySelector s)
{
    return s == kAudioStreamPropertyVirtualFormat || s == kAudioStreamPropertyPhysicalFormat;
}

static OSStatus IsPropertySettable(AudioServerPlugInDriverRef d, AudioObjectID obj, pid_t pid, const AudioObjectPropertyAddress* a, Boolean* outSettable)
{
    UInt32 size = 0;
    if (a == NULL || outSettable == NULL) return kAudioHardwareIllegalOperationError;
    OSStatus e = Prop(obj, a, 0, &size, NULL);
    if (e != 0) return e;
    *outSettable = (obj == kDevice && a->mSelector == kAudioDevicePropertyNominalSampleRate)
        || ((obj == kStreamIn || obj == kStreamOut) && IsFormat(a->mSelector));
    return 0;
}

static OSStatus GetPropertyDataSize(AudioServerPlugInDriverRef d, AudioObjectID obj, pid_t pid, const AudioObjectPropertyAddress* a,
    UInt32 qSize, const void* q, UInt32* outSize)
{
    if (a == NULL || outSize == NULL) return kAudioHardwareIllegalOperationError;
    return Prop(obj, a, 0, outSize, NULL);
}

static OSStatus GetPropertyData(AudioServerPlugInDriverRef d, AudioObjectID obj, pid_t pid, const AudioObjectPropertyAddress* a,
    UInt32 qSize, const void* q, UInt32 inSize, UInt32* outSize, void* out)
{
    if (a == NULL || outSize == NULL || out == NULL) return kAudioHardwareIllegalOperationError;
    return Prop(obj, a, inSize, outSize, out);
}

// So existe um formato: aceitar o pedido se for exatamente ele, recusar o resto.
static OSStatus SetPropertyData(AudioServerPlugInDriverRef d, AudioObjectID obj, pid_t pid, const AudioObjectPropertyAddress* a,
    UInt32 qSize, const void* q, UInt32 inSize, const void* in)
{
    if (a == NULL || in == NULL) return kAudioHardwareIllegalOperationError;
    if (obj == kDevice && a->mSelector == kAudioDevicePropertyNominalSampleRate) {
        if (inSize != sizeof(Float64)) return kAudioHardwareBadPropertySizeError;
        return *(const Float64*)in == RATE ? 0 : kAudioHardwareIllegalOperationError;
    }
    if ((obj == kStreamIn || obj == kStreamOut) && IsFormat(a->mSelector)) {
        if (inSize != sizeof(AudioStreamBasicDescription)) return kAudioHardwareBadPropertySizeError;
        const AudioStreamBasicDescription* f = (const AudioStreamBasicDescription*)in;
        Boolean same = f->mSampleRate == RATE && f->mFormatID == kAudioFormatLinearPCM
            && f->mChannelsPerFrame == CHANNELS && f->mBitsPerChannel == 32 && (f->mFormatFlags & kAudioFormatFlagIsFloat);
        return same ? 0 : kAudioDeviceUnsupportedFormatError;
    }
    UInt32 size = 0;
    OSStatus e = Prop(obj, a, 0, &size, NULL);
    return e != 0 ? e : kAudioHardwareUnsupportedOperationError;
}

// ------------------------------------------------------------------ ciclo de vida

static AudioServerPlugInDriverInterface gInterface;
static AudioServerPlugInDriverInterface* gInterfacePtr = &gInterface;
static AudioServerPlugInDriverRef gDriver = &gInterfacePtr;

static HRESULT QueryInterface(void* d, REFIID iid, LPVOID* out)
{
    if (out == NULL) return E_POINTER;
    CFUUIDRef asked = CFUUIDCreateFromUUIDBytes(NULL, iid);
    Boolean ok = asked != NULL && (CFEqual(asked, IUnknownUUID) || CFEqual(asked, kAudioServerPlugInDriverInterfaceUUID));
    if (asked) CFRelease(asked);
    if (!ok) { *out = NULL; return E_NOINTERFACE; }
    pthread_mutex_lock(&gLock);
    ++gRefs;
    pthread_mutex_unlock(&gLock);
    *out = gDriver;
    return S_OK;
}

static ULONG AddRef(void* d)
{
    pthread_mutex_lock(&gLock);
    ULONG n = ++gRefs;
    pthread_mutex_unlock(&gLock);
    return n;
}

static ULONG Release(void* d)
{
    pthread_mutex_lock(&gLock);
    ULONG n = gRefs > 0 ? --gRefs : 0;
    pthread_mutex_unlock(&gLock);
    return n;
}

static OSStatus Initialize(AudioServerPlugInDriverRef d, AudioServerPlugInHostRef host)
{
    gHost = host;
    struct mach_timebase_info tb;
    mach_timebase_info(&tb);
    gTicksPerFrame = (1000000000.0 / RATE) * (Float64)tb.denom / (Float64)tb.numer;
    return 0;
}

static OSStatus CreateDevice(AudioServerPlugInDriverRef d, CFDictionaryRef desc, const AudioServerPlugInClientInfo* c, AudioObjectID* out)
{
    return kAudioHardwareUnsupportedOperationError;
}
static OSStatus DestroyDevice(AudioServerPlugInDriverRef d, AudioObjectID dev) { return kAudioHardwareUnsupportedOperationError; }
static OSStatus AddDeviceClient(AudioServerPlugInDriverRef d, AudioObjectID dev, const AudioServerPlugInClientInfo* c) { return 0; }
static OSStatus RemoveDeviceClient(AudioServerPlugInDriverRef d, AudioObjectID dev, const AudioServerPlugInClientInfo* c) { return 0; }
static OSStatus PerformConfigChange(AudioServerPlugInDriverRef d, AudioObjectID dev, UInt64 action, void* info) { return 0; }
static OSStatus AbortConfigChange(AudioServerPlugInDriverRef d, AudioObjectID dev, UInt64 action, void* info) { return 0; }

// ------------------------------------------------------------------ audio

static void OpenSocket(void)
{
    if (gSock >= 0) return;
    int s = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
    if (s < 0) return;
    int big = 4 * 1024 * 1024, on = 1;
    setsockopt(s, SOL_SOCKET, SO_RCVBUF, &big, sizeof(big));
    setsockopt(s, SOL_SOCKET, SO_SNDBUF, &big, sizeof(big));
    setsockopt(s, SOL_SOCKET, SO_REUSEADDR, &on, sizeof(on));
    fcntl(s, F_SETFL, fcntl(s, F_GETFL, 0) | O_NONBLOCK);
    struct sockaddr_in me;
    memset(&me, 0, sizeof(me));
    me.sin_len = sizeof(me);
    me.sin_family = AF_INET;
    me.sin_port = htons(DRIVER_PORT);
    me.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    bind(s, (struct sockaddr*)&me, sizeof(me)); /* se falhar ainda da para enviar */
    gHelper = me;
    gHelper.sin_port = htons(HELPER_PORT);
    gSock = s;
}

static OSStatus StartIO(AudioServerPlugInDriverRef d, AudioObjectID dev, UInt32 client)
{
    if (dev != kDevice) return kAudioHardwareBadObjectError;
    pthread_mutex_lock(&gLock);
    if (gIOCount == 0) {
        OpenSocket();
        gStamps = 0;
        gAnchor = mach_absolute_time();
        gEpoch = (UInt32)(gAnchor >> 8) | 1u; /* muda a cada vez que a placa liga */
        gInValid = false;
    }
    ++gIOCount;
    pthread_mutex_unlock(&gLock);
    return 0;
}

static OSStatus StopIO(AudioServerPlugInDriverRef d, AudioObjectID dev, UInt32 client)
{
    if (dev != kDevice) return kAudioHardwareBadObjectError;
    pthread_mutex_lock(&gLock);
    if (gIOCount > 0) --gIOCount;
    pthread_mutex_unlock(&gLock);
    return 0;
}

static void Send(UInt32 kind, SInt64 sampleTime, const float* data, UInt32 frames)
{
    if (gSock < 0) return;
    Packet p;
    p.h.magic = MAGIC;
    p.h.epoch = gEpoch;
    p.h.anchor = gAnchor;
    p.h.ticksPerFrame = gTicksPerFrame;
    p.h.sampleTime = sampleTime;
    p.h.frames = frames;
    p.h.kind = kind;
    if (frames) memcpy(p.data, data, frames * CHANNELS * sizeof(float));
    sendto(gSock, &p, sizeof(Header) + frames * CHANNELS * sizeof(float), 0, (struct sockaddr*)&gHelper, sizeof(gHelper));
}

/* O que os programas tocaram: manda em pedacos; pedaco todo em silencio nao precisa ir. */
static void WriteOut(const float* buf, UInt32 frames, SInt64 sampleTime)
{
    Send(kKindBeat, sampleTime, NULL, 0); /* sinal de vida e relogio, mesmo em silencio */
    for (UInt32 f = 0; f < frames; f += CHUNK) {
        UInt32 n = frames - f < CHUNK ? frames - f : CHUNK;
        const float* p = buf + f * CHANNELS;
        Boolean silent = true;
        for (UInt32 i = 0; i < n * CHANNELS; i++) if (p[i] != 0.0f) { silent = false; break; }
        if (!silent) Send(kKindOut, sampleTime + f, p, n);
    }
}

static inline UInt32 Slot(SInt64 t) { return (UInt32)(((t % INRING) + INRING) % INRING); }

/* O que o auxiliar mandou da rede: guarda pela hora e entrega o trecho pedido (atrasado de MARGIN). */
static void ReadIn(float* buf, UInt32 frames, SInt64 sampleTime)
{
    Send(kKindBeat, sampleTime, NULL, 0);
    static Packet p;
    for (int i = 0; gSock >= 0 && i < 512; i++) {
        ssize_t n = recv(gSock, &p, sizeof(p), 0);
        if (n < (ssize_t)sizeof(Header)) break;
        if (p.h.magic != MAGIC || p.h.epoch != gEpoch || p.h.kind != kKindIn || p.h.frames == 0 || p.h.frames > MAX_FRAMES) continue;
        if ((size_t)n < sizeof(Header) + p.h.frames * CHANNELS * sizeof(float)) continue;
        SInt64 t = p.h.sampleTime;
        if (!gInValid || t > gInHigh + INRING / 2 || t + INRING < gInHigh) { gInHigh = t; gInValid = true; }
        for (; gInHigh < t; gInHigh++) memset(&gIn[Slot(gInHigh) * CHANNELS], 0, CHANNELS * sizeof(float)); /* buraco = silencio */
        for (UInt32 f = 0; f < p.h.frames; f++) memcpy(&gIn[Slot(t + f) * CHANNELS], &p.data[f * CHANNELS], CHANNELS * sizeof(float));
        if (t + p.h.frames > gInHigh) gInHigh = t + p.h.frames;
    }
    SInt64 a = sampleTime - MARGIN;
    for (UInt32 f = 0; f < frames; f++) {
        SInt64 t = a + f;
        if (gInValid && t < gInHigh && t + INRING > gInHigh) memcpy(buf + f * CHANNELS, &gIn[Slot(t) * CHANNELS], CHANNELS * sizeof(float));
        else memset(buf + f * CHANNELS, 0, CHANNELS * sizeof(float));
    }
}

// O relogio da placa e o relogio do Mac: um "zero" a cada RING quadros.
static OSStatus GetZeroTimeStamp(AudioServerPlugInDriverRef d, AudioObjectID dev, UInt32 client,
    Float64* outSampleTime, UInt64* outHostTime, UInt64* outSeed)
{
    if (dev != kDevice) return kAudioHardwareBadObjectError;
    pthread_mutex_lock(&gLock);
    Float64 perRing = gTicksPerFrame * (Float64)RING;
    UInt64 next = gAnchor + (UInt64)((Float64)(gStamps + 1) * perRing);
    if (mach_absolute_time() >= next) ++gStamps;
    *outSampleTime = (Float64)(gStamps * RING);
    *outHostTime = gAnchor + (UInt64)((Float64)gStamps * perRing);
    *outSeed = 1;
    pthread_mutex_unlock(&gLock);
    return 0;
}

static OSStatus WillDoIO(AudioServerPlugInDriverRef d, AudioObjectID dev, UInt32 client, UInt32 op, Boolean* outWill, Boolean* outInPlace)
{
    *outWill = op == kAudioServerPlugInIOOperationReadInput || op == kAudioServerPlugInIOOperationWriteMix;
    *outInPlace = true;
    return 0;
}

static OSStatus BeginIO(AudioServerPlugInDriverRef d, AudioObjectID dev, UInt32 client, UInt32 op, UInt32 frames, const AudioServerPlugInIOCycleInfo* c) { return 0; }
static OSStatus EndIO(AudioServerPlugInDriverRef d, AudioObjectID dev, UInt32 client, UInt32 op, UInt32 frames, const AudioServerPlugInIOCycleInfo* c) { return 0; }

static OSStatus DoIO(AudioServerPlugInDriverRef d, AudioObjectID dev, AudioObjectID stream, UInt32 client, UInt32 op, UInt32 frames,
    const AudioServerPlugInIOCycleInfo* c, void* mainBuffer, void* secondaryBuffer)
{
    if (dev != kDevice) return kAudioHardwareBadObjectError;
    if (c == NULL || mainBuffer == NULL) return 0;
    if (op == kAudioServerPlugInIOOperationReadInput && stream == kStreamIn)
        ReadIn((float*)mainBuffer, frames, (SInt64)c->mInputTime.mSampleTime);
    else if (op == kAudioServerPlugInIOOperationWriteMix && stream == kStreamOut)
        WriteOut((const float*)mainBuffer, frames, (SInt64)c->mOutputTime.mSampleTime);
    return 0;
}

static AudioServerPlugInDriverInterface gInterface = {
    NULL, QueryInterface, AddRef, Release, Initialize, CreateDevice, DestroyDevice, AddDeviceClient, RemoveDeviceClient,
    PerformConfigChange, AbortConfigChange, HasProperty, IsPropertySettable, GetPropertyDataSize, GetPropertyData, SetPropertyData,
    StartIO, StopIO, GetZeroTimeStamp, WillDoIO, BeginIO, DoIO, EndIO
};

// Ponto de entrada (nome citado no Info.plist).
void* AoIPBridge_Create(CFAllocatorRef allocator, CFUUIDRef type)
{
    return CFEqual(type, kAudioServerPlugInTypeUUID) ? gDriver : NULL;
}
