using System.Text.Json.Nodes;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Options;

namespace Zona.ProxyLib;

/// <summary>Диагностические эндпоинты, доступные только в <see cref="Environments.Development"/>.</summary>
public static class ZonaDevelopmentEndpoints
{
    /// <summary>GET: агрегированный статус Zona (прокси <c>TrainingStatusPath</c>) и флаг teach на хосте.</summary>
    public const string ZonaStatusPath = "/development/zona/status";

    /// <summary>GET: JSON-снимок всего store на Zona (<c>TrainStoreDumpPath</c>).</summary>
    public const string ZonaStorePath = "/development/zona/store";

    /// <summary>GET: ответ Zona <c>MaskProfilesPath</c>; без query — массив <c>routes</c> по каждому ключу; с <c>method</c> и <c>path</c> — один элемент.</summary>
    public const string ZonaMaskProfilesPath = "/development/zona/mask-profiles";

    /// <summary>GET: ответ Zona <c>SignatureSamplesPath</c> (сигнатуры по профилям); query как у mask-profiles.</summary>
    public const string ZonaSignatureSamplesPath = "/development/zona/signature-samples";

    /// <summary>Регистрирует маршруты; вне Development ничего не добавляется.</summary>
    public static WebApplication MapZonaDevelopmentStatus(this WebApplication app)
    {
        if (!app.Environment.IsDevelopment())
            return app;

        app.MapGet(ZonaStatusPath, GetZonaStatusAsync)
            .WithName("ZonaDevelopmentStatus")
            .WithTags("Zona")
            .SuppressZonaTeach()
            .AllowAnonymous();

        app.MapGet(ZonaStorePath, ProxyZonaStoreAsync)
            .WithName("ZonaDevelopmentStore")
            .WithTags("Zona")
            .SuppressZonaTeach()
            .AllowAnonymous();

        app.MapGet(ZonaMaskProfilesPath, ProxyZonaMaskProfilesAsync)
            .WithName("ZonaDevelopmentMaskProfiles")
            .WithTags("Zona")
            .SuppressZonaTeach()
            .AllowAnonymous();

        app.MapGet(ZonaSignatureSamplesPath, ProxyZonaSignatureSamplesAsync)
            .WithName("ZonaDevelopmentSignatureSamples")
            .WithTags("Zona")
            .SuppressZonaTeach()
            .AllowAnonymous();

        return app;
    }

    private static async Task<IResult> GetZonaStatusAsync(
        IOptions<ZonaOptions> options,
        IHttpClientFactory httpFactory,
        IZonaTeachSendGate teachGate,
        CancellationToken cancellationToken)
    {
        var o = options.Value;
        var path = o.TrainingStatusPath.TrimStart('/');
        var client = httpFactory.CreateClient(ZonaHttpNames.Client);

        try
        {
            using var response = await client.GetAsync(path, cancellationToken).ConfigureAwait(false);
            var text = await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
            JsonNode? zona = null;
            if (response.IsSuccessStatusCode)
            {
                try
                {
                    zona = JsonNode.Parse(text);
                }
                catch
                {
                    zona = JsonValue.Create(text);
                }
            }

            return Results.Ok(new ZonaDevelopmentStatusDto
            {
                TeachEnabledOnHost = teachGate.ShouldSendTeach,
                ZonaReachable = response.IsSuccessStatusCode,
                ZonaHttpStatus = (int)response.StatusCode,
                Zona = zona,
                ZonaRawBody = response.IsSuccessStatusCode ? null : text,
            });
        }
        catch (Exception ex)
        {
            return Results.Ok(new ZonaDevelopmentStatusDto
            {
                TeachEnabledOnHost = teachGate.ShouldSendTeach,
                ZonaReachable = false,
                ZonaHttpStatus = null,
                Zona = null,
                Error = ex.Message,
            });
        }
    }

    private static async Task<IResult> ProxyZonaStoreAsync(
        IOptions<ZonaOptions> options,
        IHttpClientFactory httpFactory,
        CancellationToken cancellationToken)
    {
        var rel = options.Value.TrainStoreDumpPath.TrimStart('/');
        return await ProxyZonaJsonGetAsync(httpFactory, rel, cancellationToken).ConfigureAwait(false);
    }

    private static async Task<IResult> ProxyZonaMaskProfilesAsync(
        HttpContext http,
        IOptions<ZonaOptions> options,
        IHttpClientFactory httpFactory,
        CancellationToken cancellationToken)
    {
        var baseRel = options.Value.MaskProfilesPath.TrimStart('/');
        var qs = http.Request.QueryString.HasValue ? http.Request.QueryString.Value! : string.Empty;
        var rel = baseRel + qs;
        return await ProxyZonaJsonGetAsync(httpFactory, rel, cancellationToken).ConfigureAwait(false);
    }

    private static async Task<IResult> ProxyZonaSignatureSamplesAsync(
        HttpContext http,
        IOptions<ZonaOptions> options,
        IHttpClientFactory httpFactory,
        CancellationToken cancellationToken)
    {
        var baseRel = options.Value.SignatureSamplesPath.TrimStart('/');
        var qs = http.Request.QueryString.HasValue ? http.Request.QueryString.Value! : string.Empty;
        var rel = baseRel + qs;
        return await ProxyZonaJsonGetAsync(httpFactory, rel, cancellationToken).ConfigureAwait(false);
    }

    private static async Task<IResult> ProxyZonaJsonGetAsync(
        IHttpClientFactory httpFactory,
        string pathAndQuery,
        CancellationToken cancellationToken)
    {
        var client = httpFactory.CreateClient(ZonaHttpNames.Client);
        try
        {
            using var response = await client.GetAsync(pathAndQuery, cancellationToken).ConfigureAwait(false);
            var text = await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
            return Results.Content(text, "application/json; charset=utf-8", statusCode: (int)response.StatusCode);
        }
        catch (Exception ex)
        {
            return Results.Json(
                new { error = ex.Message, path = pathAndQuery },
                statusCode: StatusCodes.Status502BadGateway);
        }
    }

    private sealed class ZonaDevelopmentStatusDto
    {
        public bool TeachEnabledOnHost { get; init; }
        public bool ZonaReachable { get; init; }
        public int? ZonaHttpStatus { get; init; }
        public JsonNode? Zona { get; init; }
        public string? ZonaRawBody { get; init; }
        public string? Error { get; init; }
    }
}
