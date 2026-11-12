// Cliente CoreAudio de teste: usa a placa "AoIP Bridge" como um programa de audio usaria.
// Toca 1 kHz nas saidas 1 (0,2) e 2 (0,1) e mostra o pico das entradas 1 a 4.
// Uso: ca_client [segundos]
#include <CoreAudio/CoreAudio.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

static float gPeak[4]; static unsigned long gCycles, gPhase; static UInt32 gFrames, gInCh, gOutCh;

static OSStatus io(AudioObjectID dev, const AudioTimeStamp* now, const AudioBufferList* in, const AudioTimeStamp* inTime,
    AudioBufferList* out, const AudioTimeStamp* outTime, void* ctx)
{
    gCycles++;
    if (in && in->mNumberBuffers > 0) {
        const AudioBuffer* b = &in->mBuffers[0];
        UInt32 ch = b->mNumberChannels, frames = b->mDataByteSize / (4 * ch);
        const float* p = (const float*)b->mData;
        gInCh = ch; gFrames = frames;
        for (UInt32 f = 0; f < frames; f++) for (UInt32 c = 0; c < 4 && c < ch; c++) { float v = fabsf(p[f * ch + c]); if (v > gPeak[c]) gPeak[c] = v; }
    }
    if (out && out->mNumberBuffers > 0) {
        AudioBuffer* b = &out->mBuffers[0];
        UInt32 ch = b->mNumberChannels, frames = b->mDataByteSize / (4 * ch);
        float* p = (float*)b->mData;
        gOutCh = ch;
        for (UInt32 f = 0; f < frames; f++, gPhase++) {
            float s = sinf(gPhase * 2 * M_PI * 1000 / 48000);
            for (UInt32 c = 0; c < ch; c++) p[f * ch + c] = c == 0 ? 0.2f * s : c == 1 ? 0.1f * s : 0;
        }
    }
    return 0;
}

int main(int argc, char** argv)
{
    int secs = argc > 1 ? atoi(argv[1]) : 10;
    AudioObjectPropertyAddress a = { kAudioHardwarePropertyTranslateUIDToDevice, kAudioObjectPropertyScopeGlobal, 0 };
    CFStringRef uid = CFSTR("AoIPBridge_UID");
    AudioObjectID dev = 0; UInt32 size = sizeof(dev);
    OSStatus e = AudioObjectGetPropertyData(kAudioObjectSystemObject, &a, sizeof(uid), &uid, &size, &dev);
    if (e != 0 || dev == 0) { printf("placa AoIP Bridge nao encontrada (erro %d)\n", (int)e); return 1; }
    Float64 rate = 0; size = sizeof(rate);
    a.mSelector = kAudioDevicePropertyNominalSampleRate;
    AudioObjectGetPropertyData(dev, &a, 0, NULL, &size, &rate);
    AudioDeviceIOProcID proc = NULL;
    e = AudioDeviceCreateIOProcID(dev, io, NULL, &proc);
    if (e == 0) e = AudioDeviceStart(dev, proc);
    if (e != 0) { printf("nao consegui iniciar o audio (erro %d)\n", (int)e); return 1; }
    printf("placa encontrada (id %u), %.0f Hz\n", (unsigned)dev, rate);
    for (int s = 1; s <= secs; s++) {
        sleep(1);
        printf("%2d s | %lu ciclos de %u quadros, %ux%u canais | pico nas entradas 1..4: %.5f %.5f %.5f %.5f\n",
            s, gCycles, (unsigned)gFrames, (unsigned)gInCh, (unsigned)gOutCh, gPeak[0], gPeak[1], gPeak[2], gPeak[3]);
        gPeak[0] = gPeak[1] = gPeak[2] = gPeak[3] = 0;
        fflush(stdout);
    }
    AudioDeviceStop(dev, proc);
    AudioDeviceDestroyIOProcID(dev, proc);
    return 0;
}
