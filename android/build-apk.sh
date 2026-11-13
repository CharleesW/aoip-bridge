#!/bin/bash
# Gera o aplicativo Android (AoIP Escuta) sem Gradle: javac + d8 + aapt2 + apksigner.
# Precisa de ~/.aoip-android com o JDK 17 e o SDK do Android (plataforma 34, build-tools 34.0.0).
set -euo pipefail
cd "$(dirname "$0")"
T="$HOME/.aoip-android"
export JAVA_HOME="$(ls -d "$T"/jdk-17*/Contents/Home | head -1)"
PATH="$JAVA_HOME/bin:$PATH"
BT="$T/sdk/build-tools/34.0.0"
PLAT="$T/sdk/platforms/android-34/android.jar"
VERSION="0.3.2"
VERSION_CODE=5
rm -rf build && mkdir -p build/logic build/test build/classes build/dex build/gen ../dist

echo "== testes da logica (sem Android)"
javac -nowarn -d build/logic src/org/aoipbridge/listener/{Sap,Rtp,JitterBuffer,Receiver}.java
javac -nowarn -cp build/logic -d build/test test/*.java
java -ea -cp build/logic:build/test LogicTest
[ "${1:-}" = "--so-testes" ] && exit 0

echo "== aplicativo"
# Recursos (icone, tema claro/escuro): compila, liga ao manifesto e gera o R.java.
"$BT/aapt2" compile --dir res -o build/res.zip
"$BT/aapt2" link -o build/base.apk --manifest AndroidManifest.xml -I "$PLAT" build/res.zip --java build/gen \
    --min-sdk-version 23 --target-sdk-version 34 --version-code "$VERSION_CODE" --version-name "$VERSION"
javac -nowarn -source 8 -target 8 -cp "$PLAT" -d build/classes src/org/aoipbridge/listener/*.java build/gen/org/aoipbridge/listener/R.java 2>&1 | grep -v "bootstrap class path\|^1 warning\|system modules" || true
"$BT/d8" --release --lib "$PLAT" --min-api 23 --output build/dex $(find build/classes -name '*.class')
(cd build/dex && zip -q -j ../base.apk classes.dex)
"$BT/zipalign" -f 4 build/base.apk build/aligned.apk
# Chave de assinatura de teste, guardada fora do projeto: a mesma chave permite atualizar o app.
KS="$T/teste.keystore"
[ -f "$KS" ] || keytool -genkeypair -keystore "$KS" -storepass android -keypass android -alias teste \
    -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=AoIP Escuta teste" >/dev/null 2>&1
OUT="../dist/AoIP-Escuta-Android-$VERSION-teste.apk"
"$BT/apksigner" sign --ks "$KS" --ks-pass pass:android --key-pass pass:android --out "$OUT" build/aligned.apk
"$BT/apksigner" verify "$OUT" && echo "pronto: dist/$(basename "$OUT")"
