// Faz o papel do sistema de audio do Mac: carrega o driver, confere as propriedades e roda
// ciclos de audio (tom nas saidas 1 e 2; mede o que volta nas entradas).
// Precisa do auxiliar rodando em autoteste (veja CLAUDE.md). Uso: AOIP_SELFTEST=2 hal_test <.../MacOS/AoIPBridge> [s]
#include <CoreAudio/AudioServerPlugIn.h>
#include <dlfcn.h>
#include <math.h>
#include <mach/mach_time.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define CHECK(c, what) do { if (!(c)) { printf("FALHOU: %s\n", what); exit(1); } } while (0)
#define FRAMES 512

static AudioServerPlugInDriverRef d;
static UInt32 u32(AudioObjectID obj, AudioObjectPropertySelector sel, AudioObjectPropertyScope scope)
{
    AudioObjectPropertyAddress a = { sel, scope, 0 };
    UInt32 v = 0xdead, size = 0;
    CHECK((*d)->HasProperty(d, obj, 0, &a), "propriedade ausente");
    CHECK((*d)->GetPropertyData(d, obj, 0, &a, 0, NULL, sizeof(v), &size, &v) == 0 && size == sizeof(v), "leitura u32");
    return v;
}

int main(int argc, char** argv)
{
    CHECK(argc > 1, "informe o caminho do binario do driver");
    int secs = argc > 2 ? atoi(argv[2]) : 10;
    void* lib = dlopen(argv[1], RTLD_NOW);
    if (!lib) { printf("dlopen: %s\n", dlerror()); return 1; }
    void* (*create)(CFAllocatorRef, CFUUIDRef) = dlsym(lib, "AoIPBridge_Create");
    CHECK(create != NULL, "ponto de entrada");
    CHECK(create(NULL, IUnknownUUID) == NULL, "tipo errado deve ser recusado");
    d = create(NULL, kAudioServerPlugInTypeUUID);
    CHECK(d != NULL, "criar driver");
    void* iface = NULL;
    CHECK((*d)->QueryInterface(d, CFUUIDGetUUIDBytes(kAudioServerPlugInDriverInterfaceUUID), &iface) == 0 && iface == d, "QueryInterface");
    CHECK((*d)->Initialize(d, NULL) == 0, "Initialize");

    CHECK(u32(1, kAudioObjectPropertyClass, 0x676c6f62) == kAudioPlugInClassID, "classe do plugin");
    CHECK(u32(2, kAudioObjectPropertyClass, 0x676c6f62) == kAudioDeviceClassID, "classe da placa");
    CHECK(u32(3, kAudioStreamPropertyDirection, 0x676c6f62) == 1 && u32(4, kAudioStreamPropertyDirection, 0x676c6f62) == 0, "direcoes");
    CHECK(u32(2, kAudioDevicePropertyZeroTimeStampPeriod, 0x676c6f62) == 16384, "periodo do relogio");
    AudioObjectPropertyAddress a = { kAudioDevicePropertyStreams, kAudioObjectPropertyScopeInput, 0 };
    AudioObjectID ids[4]; UInt32 size = 0;
    CHECK((*d)->GetPropertyData(d, 2, 0, &a, 0, NULL, sizeof(ids), &size, ids) == 0 && size == 4 && ids[0] == 3, "streams de entrada");
    a.mScope = kAudioObjectPropertyScopeGlobal;
    CHECK((*d)->GetPropertyData(d, 2, 0, &a, 0, NULL, sizeof(ids), &size, ids) == 0 && size == 8, "todas as streams");
    a.mSelector = kAudioStreamPropertyVirtualFormat;
    AudioStreamBasicDescription f;
    CHECK((*d)->GetPropertyData(d, 4, 0, &a, 0, NULL, sizeof(f), &size, &f) == 0 && f.mChannelsPerFrame == 32 && f.mSampleRate == 48000, "formato");
    CHECK((*d)->SetPropertyData(d, 4, 0, &a, 0, NULL, sizeof(f), &f) == 0, "aceita o proprio formato");
    f.mSampleRate = 44100;
    CHECK((*d)->SetPropertyData(d, 4, 0, &a, 0, NULL, sizeof(f), &f) != 0, "recusa 44100");
    a.mSelector = kAudioObjectPropertyName;
    CFStringRef name = NULL;
    CHECK((*d)->GetPropertyData(d, 2, 0, &a, 0, NULL, sizeof(name), &size, &name) == 0 && name != NULL, "nome");
    char text[64]; CFStringGetCString(name, text, sizeof(text), kCFStringEncodingUTF8);
    a.mSelector = 0x7a7a7a7a;
    CHECK(!(*d)->HasProperty(d, 2, 0, &a), "propriedade inexistente");
    CHECK(!(*d)->HasProperty(d, 99, 0, &a), "objeto inexistente");
    printf("placa: %s, 32x32, 48000 Hz; propriedades OK\n", text);

    CHECK((*d)->StartIO(d, 2, 1) == 0, "StartIO");
    CHECK(u32(2, kAudioDevicePropertyDeviceIsRunning, 0x676c6f62) == 1, "rodando");
    static float in[FRAMES * 32], out[FRAMES * 32];
    struct mach_timebase_info tb; mach_timebase_info(&tb);
    double nsPerTick = (double)tb.numer / tb.denom;
    UInt64 start = mach_absolute_time(), phase = 0, lastHost = 0; Float64 lastSample = -1;
    float peak[2] = {0, 0}, total[2] = {0, 0};
    for (int cycle = 1; cycle <= secs * 48000 / FRAMES; cycle++) {
        UInt64 due = start + (UInt64)(cycle * (FRAMES * 1e9 / 48000.0) / nsPerTick);
        while (mach_absolute_time() < due) usleep(200);
        Float64 st; UInt64 host, seed;
        CHECK((*d)->GetZeroTimeStamp(d, 2, 1, &st, &host, &seed) == 0 && st >= lastSample && host >= lastHost, "relogio andando para frente");
        lastSample = st; lastHost = host;
        // Como o sistema de verdade: a entrada e do passado recente, a saida e para daqui a pouco.
        AudioServerPlugInIOCycleInfo info; memset(&info, 0, sizeof(info));
        Float64 nowFrames = (Float64)(mach_absolute_time() - host) / (FRAMES * 1e9 / 48000.0 / nsPerTick) * FRAMES + st;
        info.mInputTime.mSampleTime = floor(nowFrames) - FRAMES;
        info.mOutputTime.mSampleTime = floor(nowFrames) + FRAMES;
        (*d)->DoIOOperation(d, 2, 3, 1, kAudioServerPlugInIOOperationReadInput, FRAMES, &info, in, NULL);
        for (int i = 0; i < FRAMES; i++) for (int c = 0; c < 2; c++) { float v = fabsf(in[i * 32 + c]); if (v > peak[c]) peak[c] = v; }
        for (int i = 0; i < FRAMES; i++, phase++) {
            float s = sinf(phase * 2 * M_PI * 1000 / 48000);
            out[i * 32] = 0.2f * s; out[i * 32 + 1] = 0.1f * s;
        }
        (*d)->DoIOOperation(d, 2, 4, 1, kAudioServerPlugInIOOperationWriteMix, FRAMES, &info, out, NULL);
        if (cycle % (48000 / FRAMES) == 0) {
            printf("%2d s | pico nas entradas: canal 1 %.3f, canal 2 %.3f | relogio %.0f quadros\n", cycle * FRAMES / 48000, peak[0], peak[1], st);
            if (cycle * FRAMES / 48000 > 3) { total[0] = fmaxf(total[0], peak[0]); total[1] = fmaxf(total[1], peak[1]); }
            peak[0] = peak[1] = 0;
        }
    }
    CHECK((*d)->StopIO(d, 2, 1) == 0, "StopIO");
    CHECK(lastSample >= (secs - 1) * 48000.0 / 16384 * 16384 - 16384, "relogio acompanhou o tempo");
    if (getenv("AOIP_SELFTEST")) CHECK(fabsf(total[0] - 0.2f) < 0.01f && fabsf(total[1] - 0.1f) < 0.01f, "o tom nao voltou com o nivel certo");
    printf("OK\n");
    return 0;
}
