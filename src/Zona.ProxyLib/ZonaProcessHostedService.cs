using System.Diagnostics;
using System.Net.Sockets;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Zona.ProxyLib;

public sealed class ZonaProcessHostedService : IHostedService, IDisposable
{
    private readonly IOptions<ZonaOptions> _options;
    private readonly ILogger<ZonaProcessHostedService> _logger;
    private Process? _process;
    private nint _job;

    public ZonaProcessHostedService(IOptions<ZonaOptions> options, ILogger<ZonaProcessHostedService> logger)
    {
        _options = options;
        _logger = logger;
    }

    public async Task StartAsync(CancellationToken cancellationToken)
    {
        var o = _options.Value;
        if (!o.AutoStartProcess)
        {
            _logger.LogInformation("Zona: AutoStartProcess=false — ожидаем уже запущенный процесс на {Listen}", o.Listen);
            return;
        }

        var path = ZonaExecutableResolver.Resolve(o);
        if (!File.Exists(path))
            throw new FileNotFoundException($"Zona: не найден исполняемый файл: {path}", path);

        var psi = new ProcessStartInfo
        {
            FileName = path,
            UseShellExecute = false,
            CreateNoWindow = true,
        };
        psi.Environment["ZONA_LISTEN"] = o.Listen;
        psi.Environment["ZONA_HOST_SESSION"] = Guid.NewGuid().ToString("N");

        var dataDir = string.IsNullOrWhiteSpace(o.DataDirectory)
            ? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Zona", "data")
            : Environment.ExpandEnvironmentVariables(o.DataDirectory.Trim());
        Directory.CreateDirectory(dataDir);
        psi.Environment["ZONA_DATA_DIR"] = dataDir;

        if (o.TeachMinSamples > 0)
            psi.Environment["ZONA_TEACH_MIN_SAMPLES"] = o.TeachMinSamples.ToString();
        if (o.TrainingCompleteTotalSamples > 0)
            psi.Environment["ZONA_TRAINING_COMPLETE_TOTAL"] = o.TrainingCompleteTotalSamples.ToString();
        if (!string.IsNullOrWhiteSpace(o.TrainingDoneMode))
            psi.Environment["ZONA_TRAINING_DONE_MODE"] = o.TrainingDoneMode.Trim();
        if (o.TrainingMinReadyRoutes > 0)
            psi.Environment["ZONA_TRAINING_MIN_READY_ROUTES"] = o.TrainingMinReadyRoutes.ToString();

        _process = Process.Start(psi);
        if (_process == null)
            throw new InvalidOperationException("Zona: Process.Start вернул null.");

        _process.EnableRaisingEvents = true;
        var procForExit = _process;
        procForExit.Exited += (_, _) =>
            _logger.LogWarning("Zona: дочерний процесс завершился (код {Code}).", procForExit.ExitCode);

        _job = ZonaWinJob.TryCreateAndAssign(_process, _logger);

        await WaitForTcpAsync(o.Listen, o.ProcessReadyTimeoutMs, cancellationToken).ConfigureAwait(false);
        _logger.LogInformation("Zona: процесс запущен, слушает {Listen}", o.Listen);
    }

    public Task StopAsync(CancellationToken cancellationToken)
    {
        if (_process is { HasExited: false })
        {
            try
            {
                _process.Kill(entireProcessTree: true);
            }
            catch (Exception ex)
            {
                _logger.LogDebug(ex, "Zona: Kill при остановке");
            }
        }

        _process?.Dispose();
        _process = null;

        ZonaWinJob.CloseJob(_job);
        _job = 0;

        return Task.CompletedTask;
    }

    public void Dispose()
    {
        _process?.Dispose();
        ZonaWinJob.CloseJob(_job);
        _job = 0;
    }

    private static async Task WaitForTcpAsync(string listen, int timeoutMs, CancellationToken ct)
    {
        if (!listen.StartsWith("http://", StringComparison.OrdinalIgnoreCase) &&
            !listen.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
            listen = "http://" + listen;

        var uri = new Uri(listen);
        var host = uri.Host;
        var port = uri.Port;

        var deadline = DateTime.UtcNow.AddMilliseconds(timeoutMs);

        while (DateTime.UtcNow < deadline)
        {
            ct.ThrowIfCancellationRequested();
            try
            {
                using var tcp = new TcpClient();
                await tcp.ConnectAsync(host, port, ct).ConfigureAwait(false);
                return;
            }
            catch
            {
                await Task.Delay(50, ct).ConfigureAwait(false);
            }
        }

        throw new TimeoutException($"Zona не ответила на TCP {host}:{port} за {timeoutMs} мс.");
    }
}
