package org.aoipbridge.listener;

/**
 * Fila estereo entre a rede e a saida de som. Guarda uma folga (`target` quadros) para
 * aguentar pacotes atrasados e corrige devagar a velocidade de leitura (ate 0,5 %) para
 * acompanhar o relogio de quem envia, que nunca e igual ao do celular.
 */
public final class JitterBuffer {
    private static final double MAX_CORRECTION = 0.005;
    private final float[] data;      // estereo intercalado
    private final int capacity;      // em quadros
    private int head, fill;          // proximo quadro a ler, quadros guardados
    private double frac;             // posicao fracionaria entre head e head+1
    private double average;          // enchimento medio (suavizado)
    private boolean primed;
    private final int target;
    public long underruns, overruns;

    public JitterBuffer(int targetFrames) {
        target = Math.max(48, targetFrames);
        capacity = target * 4 + 9600;
        data = new float[capacity * 2];
    }

    public synchronized int fill() { return fill; }
    public int target() { return target; }

    public synchronized void push(float[] in, int frames) {
        for (int f = 0; f < frames; f++) {
            if (fill == capacity) {      // lotou: descarta ate voltar a folga normal
                int drop = fill - target;
                head = (head + drop) % capacity;
                fill -= drop;
                frac = 0;
                average = target;
                overruns++;
            }
            int w = (head + fill) % capacity;
            data[w * 2] = in[f * 2];
            data[w * 2 + 1] = in[f * 2 + 1];
            fill++;
        }
    }

    /** Preenche `out` com `frames` quadros estereo; silencio enquanto a folga nao enche. */
    public synchronized void pull(float[] out, int frames) {
        if (!primed) {
            if (fill < target) {
                java.util.Arrays.fill(out, 0, frames * 2, 0f);
                return;
            }
            primed = true;
            average = fill;
            frac = 0;
        }
        average += (fill - average) * 0.01;
        double error = (average - target) / target;
        double step = 1.0 + Math.max(-MAX_CORRECTION, Math.min(MAX_CORRECTION, error * 0.02));
        // Atraso acumulado demais (rajada grande): pula de volta para a folga normal.
        if (fill > target * 3) {
            int drop = fill - target;
            head = (head + drop) % capacity;
            fill -= drop;
            average = target;
            overruns++;
        }
        for (int f = 0; f < frames; f++) {
            if (fill < 2) {              // esvaziou: silencio e recomeca a encher
                java.util.Arrays.fill(out, f * 2, frames * 2, 0f);
                primed = false;
                underruns++;
                return;
            }
            int a = head, b = (head + 1) % capacity;
            float t = (float) frac;
            out[f * 2] = data[a * 2] + (data[b * 2] - data[a * 2]) * t;
            out[f * 2 + 1] = data[a * 2 + 1] + (data[b * 2 + 1] - data[a * 2 + 1]) * t;
            frac += step;
            while (frac >= 1.0 && fill > 0) {
                frac -= 1.0;
                head = (head + 1) % capacity;
                fill--;
            }
        }
    }
}
