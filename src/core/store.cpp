// SPDX-License-Identifier: GPL-3.0-or-later

#include "store.h"

#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QRandomGenerator>
#include <QSaveFile>

namespace
{
constexpr int FormatVersion = 1;

QByteArray packEmbedding(const Embedding &e)
{
    QByteArray raw(reinterpret_cast<const char *>(e.data()), qsizetype(e.size() * sizeof(float)));
    return raw.toBase64();
}

Embedding unpackEmbedding(const QString &b64)
{
    const QByteArray raw = QByteArray::fromBase64(b64.toLatin1());
    Embedding e;
    if (raw.size() != qsizetype(Vision::EmbeddingSize * sizeof(float))) {
        return e;
    }
    e.resize(Vision::EmbeddingSize);
    memcpy(e.data(), raw.constData(), size_t(raw.size()));
    return e;
}
} // namespace

int Identity::adaptiveCount() const
{
    int n = 0;
    for (const FaceSample &s : samples) {
        n += s.pose == u"adaptive";
    }
    return n;
}

FaceStore::FaceStore(const QString &stateDir)
    : m_dir(QDir(stateDir).filePath(QStringLiteral("users")))
{
}

QString FaceStore::fileFor(uint uid) const
{
    return QDir(m_dir).filePath(QStringLiteral("%1.json").arg(uid));
}

QList<Identity> FaceStore::load(uint uid, QString *error) const
{
    QList<Identity> faces;
    QFile file(fileFor(uid));
    if (!file.exists()) {
        return faces;
    }
    if (!file.open(QIODevice::ReadOnly)) {
        if (error) {
            *error = file.errorString();
        }
        return faces;
    }

    QJsonParseError parseError;
    const QJsonDocument doc = QJsonDocument::fromJson(file.readAll(), &parseError);
    if (!doc.isObject()) {
        if (error) {
            *error = parseError.errorString();
        }
        return faces;
    }

    const QJsonArray list = doc.object().value(u"faces").toArray();
    for (const QJsonValue &v : list) {
        const QJsonObject o = v.toObject();
        Identity id;
        id.id = o.value(u"id").toString();
        id.name = o.value(u"name").toString();
        id.created = qint64(o.value(u"created").toDouble());
        id.enabled = o.value(u"enabled").toBool(true);
        id.noseT = float(o.value(u"noseT").toDouble(0.55));
        id.eyes = float(o.value(u"eyes").toDouble(0));
        for (const QJsonValue &sv : o.value(u"samples").toArray()) {
            const QJsonObject so = sv.toObject();
            FaceSample s;
            s.embedding = unpackEmbedding(so.value(u"e").toString());
            s.pose = so.value(u"pose").toString();
            s.time = qint64(so.value(u"t").toDouble());
            if (!s.embedding.empty()) {
                id.samples.append(s);
            }
        }
        if (!id.id.isEmpty() && !id.samples.isEmpty()) {
            faces.append(id);
        }
    }
    return faces;
}

bool FaceStore::save(uint uid, const QList<Identity> &faces, QString *error) const
{
    if (!QDir().mkpath(m_dir)) {
        *error = QStringLiteral("cannot create %1").arg(m_dir);
        return false;
    }
    QFile::setPermissions(m_dir, QFileDevice::ReadOwner | QFileDevice::WriteOwner | QFileDevice::ExeOwner);

    if (faces.isEmpty()) {
        remove(uid);
        return true;
    }

    QJsonArray list;
    for (const Identity &id : faces) {
        QJsonArray samples;
        for (const FaceSample &s : id.samples) {
            samples.append(QJsonObject{
                {QStringLiteral("e"), QString::fromLatin1(packEmbedding(s.embedding))},
                {QStringLiteral("pose"), s.pose},
                {QStringLiteral("t"), double(s.time)},
            });
        }
        list.append(QJsonObject{
            {QStringLiteral("id"), id.id},
            {QStringLiteral("name"), id.name},
            {QStringLiteral("created"), double(id.created)},
            {QStringLiteral("enabled"), id.enabled},
            {QStringLiteral("noseT"), double(id.noseT)},
            {QStringLiteral("eyes"), double(id.eyes)},
            {QStringLiteral("samples"), samples},
        });
    }
    const QJsonObject root{
        {QStringLiteral("version"), FormatVersion},
        {QStringLiteral("faces"), list},
    };

    QSaveFile file(fileFor(uid));
    if (!file.open(QIODevice::WriteOnly)) {
        *error = file.errorString();
        return false;
    }
    file.setPermissions(QFileDevice::ReadOwner | QFileDevice::WriteOwner);
    file.write(QJsonDocument(root).toJson(QJsonDocument::Compact));
    if (!file.commit()) {
        *error = file.errorString();
        return false;
    }
    return true;
}

bool FaceStore::remove(uint uid) const
{
    return QFile::remove(fileFor(uid));
}

QString FaceStore::newId()
{
    return QString::number(QRandomGenerator::system()->generate64() & 0xffffffffffffULL, 16);
}

FaceMatch FaceStore::bestMatch(const QList<Identity> &faces, const Embedding &e)
{
    FaceMatch best;
    for (int i = 0; i < faces.size(); ++i) {
        const Identity &id = faces.at(i);
        if (!id.enabled) {
            continue;
        }
        for (int k = 0; k < id.samples.size(); ++k) {
            const float s = Vision::similarity(id.samples.at(k).embedding, e);
            if (s > best.score) {
                best = {s, i, k};
            }
        }
    }
    return best;
}

bool FaceStore::adapt(Identity &identity, const Embedding &e, qint64 now)
{
    // Something the samples already cover adds nothing but weight.
    float closest = -1;
    for (const FaceSample &s : std::as_const(identity.samples)) {
        closest = std::max(closest, Vision::similarity(s.embedding, e));
    }
    if (closest >= 0.9f) {
        return false;
    }

    if (identity.adaptiveCount() >= MaxAdaptive) {
        for (qsizetype i = 0; i < identity.samples.size(); ++i) {
            if (identity.samples.at(i).pose == u"adaptive") {
                identity.samples.removeAt(i);
                break;
            }
        }
    }
    identity.samples.append(FaceSample{e, QStringLiteral("adaptive"), now});
    return true;
}
