using System.Net.Http.Json;
using System.Text.Json;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Zona.ProxyLib;

/// <summary>Один читатель канала — серийно шлёт POST на teach без блокировки Kestrel.</summary>
public sealed class ZonaTeachDispatchWorker : BackgroundService
{
    private readonly IZonaTeachQueue _queue;
    private readonly IZonaTeachSendGate _teachGate;
    private readonly IHttpClientFactory _httpClientFactory;
    private readonly IOptions<ZonaOptions> _options;
    private readonly ILogger<ZonaTeachDispatchWorker> _logger;

    private static readonly JsonSerializerOptions Json = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        DefaultIgnoreCondition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull,
    };

    public ZonaTeachDispatchWorker(
        IZonaTeachQueue queue,
        IZonaTeachSendGate teachGate,
        IHttpClientFactory httpClientFactory,
        IOptions<ZonaOptions> options,
        ILogger<ZonaTeachDispatchWorker> logger)
    {
        _queue = queue;
        _teachGate = teachGate;
        _httpClientFactory = httpClientFactory;
        _options = options;
        _logger = logger;
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        var reader = _queue.Reader;
        await foreach (var signal in reader.ReadAllAsync(stoppingToken).ConfigureAwait(false))
        {
            if (!_teachGate.ShouldSendTeach)
                continue;

            try
            {
                var client = _httpClientFactory.CreateClient(ZonaHttpNames.Client);
                var uri = _options.Value.BuildTeachUri();
                var body = new
                {
                    signal.Method,
                    signal.Path,
                    signal.Query,
                    signal.UnixMs,
                    durationMs = signal.DurationMs,
                    statusCode = signal.StatusCode,
                    requestLength = signal.RequestContentLength,
                    responseLength = signal.ResponseContentLength,
                    responseTtfbMs = signal.ResponseTtfbMs,
                    requestEntropyBits = signal.RequestEntropyBits,
                    requestOnesRatio = signal.RequestOnesRatio,
                    requestByteHistogram16 = signal.RequestByteHistogram16,
                    responseEntropyBits = signal.ResponseEntropyBits,
                    responseOnesRatio = signal.ResponseOnesRatio,
                    responseByteHistogram16 = signal.ResponseByteHistogram16,
                };
                using var response = await client.PostAsJsonAsync(uri, body, Json, stoppingToken).ConfigureAwait(false);
                if (!response.IsSuccessStatusCode)
                    _logger.LogDebug("Zona teach вернула {Status}", (int)response.StatusCode);
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                break;
            }
            catch (Exception ex)
            {
                _logger.LogDebug(ex, "Zona teach недоступна");
            }
        }
    }
}
