using System.Diagnostics;
using System.Threading;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;
using Microsoft.AspNetCore.Mvc.Controllers;
using Microsoft.AspNetCore.Routing;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Zona.ProxyLib;

public sealed class ZonaTeachMiddleware
{
    private readonly RequestDelegate _next;
    private readonly IZonaTeachQueue _queue;
    private readonly IZonaTeachSendGate _teachGate;
    private readonly IOptions<ZonaOptions> _options;
    private readonly ILogger<ZonaTeachMiddleware> _logger;

    public ZonaTeachMiddleware(
        RequestDelegate next,
        IZonaTeachQueue queue,
        IZonaTeachSendGate teachGate,
        IOptions<ZonaOptions> options,
        ILogger<ZonaTeachMiddleware> logger)
    {
        _next = next;
        _queue = queue;
        _teachGate = teachGate;
        _options = options;
        _logger = logger;
    }

    public async Task InvokeAsync(HttpContext context)
    {
        var recordTeach = ShouldRecordTeach(context);

        var path = context.Request.Path.Value ?? "/";
        string? queryRaw = context.Request.QueryString.HasValue ? context.Request.QueryString.Value : null;
        long unixMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        var sw = Stopwatch.StartNew();
        var ttfbCapture = new ResponseTtfbMsCapture();
        context.Response.OnStarting(
            _ =>
            {
                ttfbCapture.TryRecordFirst(sw.ElapsedMilliseconds);
                return Task.CompletedTask;
            },
            null!);

        var maxStats = Math.Max(0, _options.Value.TeachBodyStatsMaxBytes);
        context.Request.EnableBuffering();

        var originalBody = context.Response.Body;
        TeachResponseBodyStatsStream? captureWrap = null;
        if (maxStats > 0)
        {
            captureWrap = new TeachResponseBodyStatsStream(originalBody, maxStats);
            context.Response.Body = captureWrap;
        }

        try
        {
            await _next(context).ConfigureAwait(false);
        }
        finally
        {
            sw.Stop();

            if (recordTeach && _teachGate.ShouldSendTeach)
            {
                TrafficPatternStats? reqPattern = null;
                TrafficPatternStats? resPattern = captureWrap?.GetCapturedStats();

                if (maxStats > 0)
                {
                    try
                    {
                        reqPattern = await ZonaTeachBodyStats.FromRequestBodyAsync(
                            context.Request.Body,
                            maxStats,
                            context.RequestAborted).ConfigureAwait(false);
                    }
                    catch (OperationCanceledException)
                    {
                        reqPattern = null;
                    }
                    catch (Exception ex)
                    {
                        _logger.LogDebug(ex, "Zona: не удалось посчитать паттерн тела запроса для teach");
                    }
                }

                var durationMs = ElapsedMsAtLeastOneIfNonZero(sw);
                long? responseLen = context.Response.ContentLength;
                if (responseLen is null && captureWrap is not null && captureWrap.TotalBytesWritten > 0)
                    responseLen = captureWrap.TotalBytesWritten;

                long? requestLen = context.Request.ContentLength;
                if (requestLen is null && HttpMethods.IsGet(context.Request.Method))
                    requestLen = 0;
                if (requestLen is null && HttpMethods.IsHead(context.Request.Method))
                    requestLen = 0;

                long? ttfbMs = ttfbCapture.Milliseconds;
                if (ttfbMs is null && context.Response.HasStarted && durationMs > 0)
                    ttfbMs = durationMs;

                if (captureWrap is not null)
                    context.Response.Body = originalBody;

                var signal = new ZonaTeachSignal(
                    context.Request.Method,
                    path,
                    queryRaw,
                    unixMs,
                    durationMs,
                    context.Response.StatusCode,
                    requestLen,
                    responseLen,
                    ttfbMs,
                    reqPattern?.EntropyBitsPerByte,
                    reqPattern?.OnesRatio,
                    reqPattern?.ByteHistogram16,
                    resPattern?.EntropyBitsPerByte,
                    resPattern?.OnesRatio,
                    resPattern?.ByteHistogram16);

                if (!_queue.TryEnqueue(signal))
                    _logger.LogWarning("Zona: не удалось поставить teach в очередь");
            }
            else if (captureWrap is not null)
            {
                context.Response.Body = originalBody;
            }
        }
    }

    /// <summary>Stopwatch в мс даёт 0 при &lt;1 ms; для teach сохраняем минимум 1 мс, если запрос реально занял время.</summary>
    private static long ElapsedMsAtLeastOneIfNonZero(Stopwatch sw)
    {
        var ms = sw.ElapsedMilliseconds;
        if (ms == 0 && sw.ElapsedTicks > 0)
            return 1;
        return ms;
    }

    /// <summary>
    /// Обучение только для «публичного API» по метаданным сопоставлённого эндпоинта.
    /// Нужен <c>UseRouting()</c> до этого middleware, иначе <see cref="HttpContext.GetEndpoint"/> будет null.
    /// </summary>
    private static bool ShouldRecordTeach(HttpContext context)
    {
        var endpoint = context.GetEndpoint();
        if (endpoint is null)
            return false;

        if (endpoint.Metadata.GetMetadata<ISuppressZonaTeachMetadata>() is not null)
            return false;

        if (endpoint.Metadata.GetMetadata<IExcludeFromDescriptionMetadata>() is { ExcludeFromDescription: true })
            return false;

        if (endpoint.Metadata.GetMetadata<ControllerActionDescriptor>() is { } cad)
        {
            var t = cad.ControllerTypeInfo;
            return t.IsDefined(typeof(ApiControllerAttribute), inherit: true);
        }

        return endpoint.Metadata.GetMetadata<IHttpMethodMetadata>() is not null;
    }

    /// <summary>Первое срабатывание OnStarting фиксирует TTFB; повторные вызовы игнорируются.</summary>
    private sealed class ResponseTtfbMsCapture
    {
        private long _msOrUnset = -1;

        internal void TryRecordFirst(long elapsedMs)
        {
            Interlocked.CompareExchange(ref _msOrUnset, elapsedMs, -1);
        }

        internal long? Milliseconds
        {
            get
            {
                long v = Volatile.Read(ref _msOrUnset);
                return v < 0 ? null : v;
            }
        }
    }
}
