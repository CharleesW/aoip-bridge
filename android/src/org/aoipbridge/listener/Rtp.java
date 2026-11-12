package org.aoipbridge.listener;

/** Cabecalho RTP e audio linear (L24/L16, big-endian). */
public final class Rtp {
    public int pt, seq, offset, length;
    public long timestamp;

    /** Preenche os campos; false se o pacote nao for RTP valido. */
    public boolean parse(byte[] b, int len) {
        if (len < 12 || (b[0] & 0xC0) != 0x80) return false;
        int pos = 12 + (b[0] & 0x0F) * 4;
        if ((b[0] & 0x10) != 0) {                       // extensao de cabecalho
            if (pos + 4 > len) return false;
            pos += 4 + (((b[pos + 2] & 0xFF) << 8) | (b[pos + 3] & 0xFF)) * 4;
        }
        int end = len;
        if ((b[0] & 0x20) != 0) end -= b[len - 1] & 0xFF; // enchimento
        if (pos > end) return false;
        pt = b[1] & 0x7F;
        seq = ((b[2] & 0xFF) << 8) | (b[3] & 0xFF);
        timestamp = ((long) (b[4] & 0xFF) << 24) | ((b[5] & 0xFF) << 16) | ((b[6] & 0xFF) << 8) | (b[7] & 0xFF);
        offset = pos;
        length = end - pos;
        return true;
    }

    /**
     * Tira dois canais do audio e grava em `out` como estereo intercalado (-1..1).
     * `left`/`right` contam a partir de 0; fora do fluxo = silencio. Devolve os quadros.
     */
    public static int decode(byte[] b, int off, int len, int bits, int channels, int left, int right, float[] out) {
        int bytes = bits / 8, frame = bytes * channels;
        if (frame <= 0) return 0;
        int frames = Math.min(len / frame, out.length / 2);
        for (int f = 0; f < frames; f++) {
            int base = off + f * frame;
            out[f * 2] = left >= 0 && left < channels ? sample(b, base + left * bytes, bits) : 0f;
            out[f * 2 + 1] = right >= 0 && right < channels ? sample(b, base + right * bytes, bits) : 0f;
        }
        return frames;
    }

    private static float sample(byte[] b, int p, int bits) {
        if (bits == 24) return ((b[p] << 16) | ((b[p + 1] & 0xFF) << 8) | (b[p + 2] & 0xFF)) / 8388608f;
        return ((b[p] << 8) | (b[p + 1] & 0xFF)) / 32768f;
    }
}
