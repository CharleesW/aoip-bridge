package org.aoipbridge.listener;

/**
 * Fila estereo entre a rede e a saida de som. Guarda uma folga (`target` quadros) para
 * aguentar pacotes atrasados e corrige devagar a velocidade de leitura (ate 0,5 %) para
 * acompanhar o relogio de quem envia, que nunca e igual ao do celular.
 *
 * Folga automatica: com `maxFrames` maior que a folga inicial, cada vez que o som falha a
 * folga cresce (metade a mais, no minimo 10 ms) ate `maxFrames`. Ela nunca diminui sozinha:
 * se a rede melhorar, basta parar e ouvir de novo.
 */
public final class JitterBuffer {
    private static final double MAX_CORRECTION = 0.005;
    private static final int MIN_RAISE = 480; // 10 ms
    private final float[] data;      // estereo intercalado
    private final int capacity;      // em quadros
    private final int maxTarget;
    private int head, fill;          // proximo quadro a ler, quadros guardados
    private double frac;             // posicao fracionaria entre head e head+1
    private double average;          // enchimento medio (suavizado)
    private boolean primed;
    private volatile int target;
    public volatile long underruns, overruns, raises;

    /** Folga fixa. */
    public JitterBuffer(int targetFrames) { this(targetFrames, 0); }

    /** Folga que comeca em `targetFrames` e pode crescer ate `maxFrames` (0 = fixa). */
    public JitterBuffer(int targetFrames, int maxFrames) {
        target = Math.max(48, targetFrames);
        maxTarget = Math.max(target, maxFrames);
        capacity = maxTarget * 4 + 9600;
        data = new float[capacity * 2];
    }

    public synchronized int fill() { return fill; }
    public int target() { return target; }
    public boolean automatic() { return maxTarget > target || raises > 0; }

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
                if (target < maxTarget) {
                    target = Math.min(maxTarget, target + Math.max(MIN_RAISE, target / 2));
                    raises++;
                }
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
