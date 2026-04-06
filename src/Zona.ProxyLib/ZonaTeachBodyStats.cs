using System.Buffers;
using System.Numerics;

namespace Zona.ProxyLib;

/// <summary>
/// Агрегаты по байтам тела — энтропия, доля единичных бит и грубая форма распределения байтов
/// (полезно для сопоставления с наблюдаемыми паттернами TLS record sizes / сессионного трафика).
/// </summary>
public static class ZonaTeachBodyStats
{
    /// <summary>Стойкий к выборке короткого префикса потока (до <paramref name="maxBytes"/> байт).</summary>
    public static TrafficPatternStats? FromHistogram(int[] hist256, int bytesSampled, long oneBitsTotal)
    {
        if (bytesSampled <= 0 || hist256.Length != 256)
            return null;

        double n = bytesSampled;
        double entropy = 0.0;
        for (int c = 0; c < 256; c++)
        {
            int cnt = hist256[c];
            if (cnt == 0)
                continue;
            double p = cnt / n;
            entropy -= p * Math.Log(p, 2);
        }

        double onesRatio = oneBitsTotal / (n * 8.0);
        var h16 = new double[16];
        for (int b = 0; b < 16; b++)
        {
            int from = b * 16;
            int sum = 0;
            for (int j = 0; j < 16; j++)
                sum += hist256[from + j];
            h16[b] = sum / n;
        }

        return new TrafficPatternStats(
            Math.Round(entropy, 4),
            Math.Round(onesRatio, 6),
            h16);
    }

    /// <summary>Считает статистики по первым <paramref name="maxBytes"/> байтам тела запроса (поток должен поддерживать Seek).</summary>
    public static async Task<TrafficPatternStats?> FromRequestBodyAsync(Stream body, int maxBytes, CancellationToken cancellationToken)
    {
        if (!body.CanSeek || maxBytes <= 0)
            return null;

        body.Position = 0;
        var hist = new int[256];
        int sampled = 0;
        long oneBits = 0;
        var buf = ArrayPool<byte>.Shared.Rent(Math.Min(65536, maxBytes));

        try
        {
            while (sampled < maxBytes)
            {
                int want = Math.Min(buf.Length, maxBytes - sampled);
                int read = await body.ReadAsync(buf.AsMemory(0, want), cancellationToken).ConfigureAwait(false);
                if (read == 0)
                    break;
                AddChunk(buf.AsSpan(0, read), hist, ref oneBits);
                sampled += read;
            }
        }
        finally
        {
            ArrayPool<byte>.Shared.Return(buf);
            body.Position = 0;
        }

        return FromHistogram(hist, sampled, oneBits);
    }

    internal static void AddChunk(ReadOnlySpan<byte> chunk, int[] hist256, ref long oneBitsTotal)
    {
        for (int i = 0; i < chunk.Length; i++)
        {
            byte b = chunk[i];
            hist256[b]++;
            oneBitsTotal += BitOperations.PopCount(b);
        }
    }
}

/// <summary>Агрегаты по префиксу потока: энтропия, доля единичных бит, 16-биновая гистограмма байтов.</summary>
public readonly record struct TrafficPatternStats(
    double EntropyBitsPerByte,
    double OnesRatio,
    double[] ByteHistogram16);
