namespace Zona.ProxyLib;

/// <summary>Сигнал teach после завершения обработки запроса в пайплайне (тайминг, размеры, статистика «формы» байтов для сопоставления с TLS-трафиком).</summary>
public readonly record struct ZonaTeachSignal(
    string Method,
    string Path,
    /// <summary>Сырая query с Kestrel (часто с ведущим «?»); каноникализация — в Zona (Rust).</summary>
    string? Query,
    long UnixMs,
    long DurationMs,
    int StatusCode,
    long? RequestContentLength,
    long? ResponseContentLength,
    /// <summary>Мс от начала запроса до колбэка OnStarting ответа (серверный TTFB, до ухода ответа в сокет).</summary>
    long? ResponseTtfbMs,
    /// <summary>Энтропия Шеннона на байт (0…8) по префиксу тела запроса.</summary>
    double? RequestEntropyBits,
    /// <summary>Доля единичных бит по учтённым байтам запроса.</summary>
    double? RequestOnesRatio,
    /// <summary>Доли байтов в 16 диапазонах (0…15 … 240…255).</summary>
    double[]? RequestByteHistogram16,
    double? ResponseEntropyBits,
    double? ResponseOnesRatio,
    double[]? ResponseByteHistogram16);
