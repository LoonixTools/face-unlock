// SPDX-License-Identifier: GPL-3.0-or-later
//
// The face store: what goes in comes back out, matching picks the right
// sample, and learning from unlocks stays within its bounds.

#include "store.h"

#include <QFileInfo>
#include <QTemporaryDir>

#include <cmath>
#include <cstdio>
#include <random>

namespace
{
int failures = 0;

void check(bool ok, const char *what)
{
    std::printf("%s  %s\n", ok ? "ok  " : "FAIL", what);
    failures += !ok;
}

Embedding randomUnit(std::mt19937 &rng)
{
    std::normal_distribution<float> n;
    Embedding e(Vision::EmbeddingSize);
    float norm = 0;
    for (float &v : e) {
        v = n(rng);
        norm += v * v;
    }
    for (float &v : e) {
        v /= std::sqrt(norm);
    }
    return e;
}

// A vector about `similarity` away from `base`.
Embedding near(const Embedding &base, float similarity, std::mt19937 &rng)
{
    Embedding other = randomUnit(rng);
    Embedding out(base.size());
    const float w = std::sqrt(1 - similarity * similarity);
    float norm = 0;
    for (size_t i = 0; i < base.size(); ++i) {
        out[i] = similarity * base[i] + w * other[i];
        norm += out[i] * out[i];
    }
    for (float &v : out) {
        v /= std::sqrt(norm);
    }
    return out;
}
} // namespace

int main()
{
    std::mt19937 rng(7);
    QTemporaryDir dir;
    const FaceStore store(dir.path());

    Identity a;
    a.id = FaceStore::newId();
    a.name = QStringLiteral("Felix mit Brille");
    a.created = 1758550000;
    a.noseT = 0.57f;
    a.eyes = 0.41f;
    const Embedding faceA = randomUnit(rng);
    for (int i = 0; i < 5; ++i) {
        a.samples.append({near(faceA, 0.8f, rng), QStringLiteral("center"), 1758550000 + i});
    }

    Identity b;
    b.id = FaceStore::newId();
    b.name = QStringLiteral("Other");
    const Embedding faceB = randomUnit(rng);
    b.samples.append({faceB, QStringLiteral("center"), 1});

    QString error;
    check(store.save(1000, {a, b}, &error), "save");
    const QString file = dir.filePath(QStringLiteral("users/1000.json"));
    check(QFileInfo(file).permissions() == (QFileDevice::ReadOwner | QFileDevice::WriteOwner | QFileDevice::ReadUser | QFileDevice::WriteUser),
          "the file is readable by its owner only");

    const QList<Identity> loaded = store.load(1000, &error);
    check(loaded.size() == 2, "both faces come back");
    check(loaded.value(0).name == a.name && loaded.value(0).samples.size() == 5, "names and samples survive");
    check(std::abs(loaded.value(0).noseT - 0.57f) < 1e-6f && std::abs(loaded.value(0).eyes - 0.41f) < 1e-6f, "attention baselines survive");
    check(Vision::similarity(loaded.value(0).samples.value(0).embedding, a.samples.value(0).embedding) > 0.9999f, "embeddings survive exactly");
    check(store.load(1001).isEmpty(), "another user has no faces");

    const FaceMatch m = FaceStore::bestMatch(loaded, near(faceA, 0.9f, rng));
    check(m.identity == 0 && m.score > 0.6f, "the right face matches");
    const FaceMatch stranger = FaceStore::bestMatch(loaded, randomUnit(rng));
    check(stranger.score < 0.4f, "a stranger does not");

    QList<Identity> disabled = loaded;
    disabled[0].enabled = false;
    check(FaceStore::bestMatch(disabled, near(faceA, 0.9f, rng)).identity != 0, "a face that is turned off never matches");

    Identity learning = loaded.value(0);
    check(!FaceStore::adapt(learning, learning.samples.value(0).embedding, 5), "nothing new: nothing learned");
    int added = 0;
    for (int i = 0; i < 40; ++i) {
        added += FaceStore::adapt(learning, near(faceA, 0.6f, rng), 10 + i);
    }
    check(added > FaceStore::MaxAdaptive, "new looks are learned");
    check(learning.adaptiveCount() == FaceStore::MaxAdaptive, "but only so many of them are kept");
    int setup = 0;
    for (const FaceSample &s : learning.samples) {
        setup += s.pose != u"adaptive";
    }
    check(setup == 5, "the samples from the setup are never replaced");

    check(store.save(1000, {}, &error) && !QFileInfo::exists(file), "saving nothing removes the file");

    std::printf("\n%s\n", failures ? "SOME CHECKS FAILED" : "all checks passed");
    return failures ? 1 : 0;
}
