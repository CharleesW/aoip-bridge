package org.aoipbridge.listener;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.Path;
import android.graphics.RectF;
import android.view.View;

/** Desenhos da identidade visual: simbolo da marca, icones de estado e medidor de nivel. */
final class Widgets {
    private Widgets() { }

    /** Simbolo AOIP-Bridge (assets/brand/symbol-*.svg), desenhado em 24 x 24 unidades. */
    static final class BrandMark extends View {
        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Path path = new Path();
        private final int ink, gap;

        BrandMark(Context c, int ink, int gap) {
            super(c);
            this.ink = ink;
            this.gap = gap;
            setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        }

        @Override protected void onDraw(Canvas cv) {
            float s = Math.min(getWidth(), getHeight()) / 24f;
            cv.save();
            cv.translate((getWidth() - 24 * s) / 2, (getHeight() - 24 * s) / 2);
            cv.scale(s, s);
            path.reset();
            path.moveTo(4, 18); path.lineTo(8, 18);
            path.cubicTo(12.5f, 18, 11.5f, 6, 16, 6);
            path.lineTo(20, 6);
            paint.setStyle(Paint.Style.STROKE); paint.setStrokeWidth(2.2f); paint.setStrokeCap(Paint.Cap.ROUND); paint.setColor(ink);
            cv.drawPath(path, paint);
            paint.setStyle(Paint.Style.FILL);
            cv.drawCircle(4, 18, 2.6f, paint);
            cv.drawCircle(20, 6, 2.6f, paint);
            paint.setColor(gap); cv.drawCircle(12, 12, 3.6f, paint);       // contorno do no central
            paint.setColor(Palette.BRAND); cv.drawCircle(12, 12, 3f, paint);
            cv.restore();
        }
    }

    /** Icones de estado do painel (constante ICON em core/src/ui.html), em 16 x 16 unidades. */
    static final class StatusIcon extends View {
        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Path path = new Path();
        private String kind = "";
        private int color;

        StatusIcon(Context c) {
            super(c);
            setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO); // o texto ao lado ja diz o estado
        }

        void set(String kind, int color) {
            this.kind = kind;
            this.color = color;
            invalidate();
        }

        @Override protected void onDraw(Canvas cv) {
            float s = Math.min(getWidth(), getHeight()) / 16f;
            cv.save();
            cv.scale(s, s);
            paint.setStyle(Paint.Style.STROKE); paint.setStrokeWidth(1.75f);
            paint.setStrokeCap(Paint.Cap.ROUND); paint.setStrokeJoin(Paint.Join.ROUND); paint.setColor(color);
            path.reset();
            switch (kind) {
                case "ok":
                    cv.drawCircle(8, 8, 6.25f, paint);
                    path.moveTo(5.4f, 8.2f); path.lineTo(7.2f, 10); path.lineTo(10.7f, 6.3f);
                    break;
                case "bad":
                    cv.drawCircle(8, 8, 6.25f, paint);
                    path.moveTo(6, 6); path.lineTo(10, 10); path.moveTo(10, 6); path.lineTo(6, 10);
                    break;
                case "warn":
                    path.moveTo(8, 2.3f); path.lineTo(14.1f, 13.1f); path.lineTo(1.9f, 13.1f); path.close();
                    path.moveTo(8, 6.5f); path.lineTo(8, 9.5f); path.moveTo(8, 11.5f); path.lineTo(8, 11.6f);
                    break;
                case "info": {
                    RectF r = new RectF(2.5f, 2.5f, 13.5f, 13.5f);
                    path.addArc(r, -16, -131.5f);
                    path.addArc(r, 164, -127);
                    path.moveTo(3.4f, 2.6f); path.lineTo(3.4f, 5.4f); path.lineTo(6.2f, 5.4f);
                    path.moveTo(12.6f, 13.4f); path.lineTo(12.6f, 10.6f); path.lineTo(9.8f, 10.6f);
                    break;
                }
                default:
                    cv.drawCircle(8, 8, 6.25f, paint);
                    path.moveTo(5.4f, 8); path.lineTo(10.6f, 8);
            }
            cv.drawPath(path, paint);
            cv.restore();
        }
    }

    /** Medidor de -60 a 0 dB. Acima de -1 dB fica vermelho e ganha uma marca no fim. */
    static final class Meter extends View {
        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final RectF r = new RectF();
        private final Palette c;
        private float db = -100;

        Meter(Context ctx, Palette c) {
            super(ctx);
            this.c = c;
        }

        /** `peak` de 0 a 1. */
        void setPeak(float peak) {
            db = peak <= 0.000001f ? -100 : (float) (20 * Math.log10(peak));
            invalidate();
        }

        float db() { return db; }

        @Override protected void onDraw(Canvas cv) {
            float h = getHeight(), w = getWidth(), rad = h / 2;
            paint.setColor(c.meterTrack);
            r.set(0, 0, w, h);
            cv.drawRoundRect(r, rad, rad, paint);
            if (db <= -60) return;
            boolean clip = db > -1;
            float frac = Math.min(1, (db + 60) / 60);
            paint.setColor(clip ? c.err : c.route);
            r.set(0, 0, Math.max(h, w * frac), h);
            cv.drawRoundRect(r, rad, rad, paint);
            if (clip) {
                paint.setColor(c.text);
                float m = h * 0.35f;
                cv.drawRect(w - m - rad * 0.6f, 0, w - rad * 0.6f, h, paint);
            }
        }
    }
}
