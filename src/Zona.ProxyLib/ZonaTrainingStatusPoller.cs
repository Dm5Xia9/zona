using System.Text.Json;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Zona.ProxyLib;

/// <summary>Опрашивает GET /training-status у Zona и отключает teach после <c>trainingComplete</c>.</summary>
public sealed class ZonaTrainingStatusPoller : BackgroundService
{
    private static readonly JsonSerializerOptions Json = new()
    {
        PropertyNameCaseInsensitive = true,
    };

    private readonly IHttpClientFactory _httpClientFactory;
    private readonly IOptionsMonitor<ZonaOptions> _options;
    private readonly ZonaTeachSendGate _gate;
    private readonly ILogger<ZonaTrainingStatusPoller> _logger;

    public ZonaTrainingStatusPoller(
        IHttpClientFactory httpClientFactory,
        IOptionsMonitor<ZonaOptions> options,
        ZonaTeachSendGate gate,
        ILogger<ZonaTrainingStatusPoller> logger)
    {
        _httpClientFactory = httpClientFactory;
        _options = options;
        _gate = gate;
        _logger = logger;
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        await Task.Delay(TimeSpan.FromMilliseconds(400), stoppingToken).ConfigureAwait(false);

        while (!stoppingToken.IsCancellationRequested)
        {
            var refresh = TimeSpan.FromSeconds(Math.Max(1, _options.CurrentValue.TrainingStatusRefreshSeconds));
            try
            {
                if (_gate.ShouldSendTeach)
                {
                    var client = _httpClientFactory.CreateClient(ZonaHttpNames.Client);
                    var path = _options.CurrentValue.TrainingStatusPath.TrimStart('/');
                    using var response = await client.GetAsync(path, stoppingToken).ConfigureAwait(false);
                    if (response.IsSuccessStatusCode)
                    {
                        await using var stream = await response.Content.ReadAsStreamAsync(stoppingToken).ConfigureAwait(false);
                        var dto = await JsonSerializer.DeserializeAsync<ZonaTrainingStatusDto>(stream, Json, stoppingToken).ConfigureAwait(false);
                        if (dto?.TrainingComplete == true)
                        {
                            _gate.SetTrainingComplete();
                            _logger.LogInformation("Zona: обучение завершено (trainingComplete), teach отключён.");
                        }
                    }
                }
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                break;
            }
            catch (Exception ex)
            {
                _logger.LogDebug(ex, "Zona: опрос training-status");
            }

            try
            {
                await Task.Delay(refresh, stoppingToken).ConfigureAwait(false);
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                break;
            }
        }
    }

    private sealed class ZonaTrainingStatusDto
    {
        public bool TrainingComplete { get; set; }
    }
}
