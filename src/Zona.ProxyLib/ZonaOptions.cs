namespace Zona.ProxyLib;

public sealed class ZonaOptions
{
    public const string SectionName = "Zona";

    /// <summary>Запускать бинарник <c>zona</c> вместе с приложением.</summary>
    public bool AutoStartProcess { get; set; } = true;

    /// <summary>Пусто: <c>ZONA_EXECUTABLE</c>, затем <c>zona.exe</c> рядом со сборкой.</summary>
    public string ExecutablePath { get; set; } = "";

    /// <summary><c>host:port</c> для <c>ZONA_LISTEN</c> и для HTTP-клиента к teach.</summary>
    public string Listen { get; set; } = "127.0.0.1:8787";

    /// <summary>Относительный путь на сервере Zona (по умолчанию <c>/teach</c>).</summary>
    public string TeachPath { get; set; } = "/teach";

    /// <summary>Ожидание TCP до этого таймаута после старта процесса.</summary>
    public int ProcessReadyTimeoutMs { get; set; } = 8000;

    /// <summary>Сколько первых байт тел запроса/ответа учитывать в энтропии и гистограмме (TLS-маркеры по паттерну потока).</summary>
    public int TeachBodyStatsMaxBytes { get; set; } = 262144;

    /// <summary>Каталог для <c>ZONA_DATA_DIR</c> (train_store.json). Пусто — <c>%LocalAppData%\Zona\data</c>.</summary>
    public string DataDirectory { get; set; } = "";

    /// <summary>Путь GET статуса обучения на Zona.</summary>
    public string TrainingStatusPath { get; set; } = "/training-status";

    /// <summary>Путь GET полного снимка teach-store на Zona (проксируется в development).</summary>
    public string TrainStoreDumpPath { get; set; } = "/train-store";

    /// <summary>Путь GET mask-profiles на Zona: без query — профили по каждому маршруту; с <c>method</c> и <c>path</c> — один маршрут.</summary>
    public string MaskProfilesPath { get; set; } = "/mask-profiles";

    /// <summary>Путь GET синтезированных сигнатур запроса/ответа на Zona (как у mask-profiles по query).</summary>
    public string SignatureSamplesPath { get; set; } = "/signature-samples";

    /// <summary>Интервал опроса статуса обучения Zona (с).</summary>
    public int TrainingStatusRefreshSeconds { get; set; } = 5;

    /// <summary>Если &gt; 0 — в процесс Zona передаётся <c>ZONA_TEACH_MIN_SAMPLES</c>.</summary>
    public int TeachMinSamples { get; set; } = 0;

    /// <summary>Если &gt; 0 — <c>ZONA_TRAINING_COMPLETE_TOTAL</c> (только при <c>TrainingDoneMode=total</c>).</summary>
    public int TrainingCompleteTotalSamples { get; set; } = 0;

    /// <summary><c>ZONA_TRAINING_DONE_MODE</c>: <c>all_routes</c> (не менее <see cref="TrainingMinReadyRoutes"/> ключей с ≥ TeachMinSamples) или <c>total</c>.</summary>
    public string TrainingDoneMode { get; set; } = "all_routes";

    /// <summary>Если &gt; 0 — <c>ZONA_TRAINING_MIN_READY_ROUTES</c> (режим <c>all_routes</c>).</summary>
    public int TrainingMinReadyRoutes { get; set; } = 0;

    public static Uri BuildBaseUri(string listen)
    {
        var s = listen.Trim();
        if (!s.StartsWith("http://", StringComparison.OrdinalIgnoreCase) &&
            !s.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
            s = "http://" + s;
        var u = new Uri(s);
        return new Uri($"{u.Scheme}://{u.Authority}/");
    }

    public Uri BuildTeachUri() =>
        new(BuildBaseUri(Listen), TeachPath.TrimStart('/'));

    public Uri BuildTrainingStatusUri() =>
        new(BuildBaseUri(Listen), TrainingStatusPath.TrimStart('/'));
}
