using Microsoft.AspNetCore.Http.HttpResults;
using Microsoft.AspNetCore.Mvc;
using Zona.ProxyLib;

namespace Zona.Api;

/// <summary>Демо-эндпоинты для обучения Zona: список с фильтрами, сущность по id, константы.</summary>
public static class DemoApiEndpoints
{
    private static readonly string[] Categories = ["electronics", "books", "food", "home", "sports"];
    private static readonly ProductDto[] Catalog = BuildCatalog();

    public static WebApplication MapDemoApi(this WebApplication app)
    {
        app.MapGet("/api/items", GetItemsList)
            .WithName("ListItems")
            .WithTags("Demo")
            .WithOpenApi();

        app.MapGet("/api/items/{id:int}", GetItemById)
            .WithName("GetItemById")
            .WithTags("Demo")
            .WithOpenApi();

        app.MapGet("/api/constants", GetConstants)
            .WithName("GetAppConstants")
            .WithTags("Demo")
            .WithOpenApi();

        if (app.Environment.IsDevelopment())
        {
            app.MapPost("/development/simulate-api-traffic", SimulateApiTrafficAsync)
                .WithName("SimulateApiTraffic")
                .WithTags("Development")
                .SuppressZonaTeach()
                .WithOpenApi();
        }

        return app;
    }

    private static Ok<PagedItemsResponse> GetItemsList(
        [FromQuery] string? category,
        [FromQuery] decimal? minPrice,
        [FromQuery] decimal? maxPrice,
        [FromQuery] int page = 1,
        [FromQuery] int pageSize = 10)
    {
        page = Math.Max(1, page);
        pageSize = Math.Clamp(pageSize, 1, 50);

        IEnumerable<ProductDto> q = Catalog;
        if (!string.IsNullOrWhiteSpace(category))
            q = q.Where(p => p.Category.Equals(category.Trim(), StringComparison.OrdinalIgnoreCase));
        if (minPrice is not null)
            q = q.Where(p => p.Price >= minPrice);
        if (maxPrice is not null)
            q = q.Where(p => p.Price <= maxPrice);

        var list = q.ToList();
        var total = list.Count;
        var slice = list.Skip((page - 1) * pageSize).Take(pageSize).ToList();

        return TypedResults.Ok(new PagedItemsResponse(
            slice,
            total,
            page,
            pageSize,
            (int)Math.Ceiling(total / (double)pageSize)));
    }

    private static Results<Ok<ProductDto>, NotFound> GetItemById([FromRoute] int id)
    {
        var p = Catalog.FirstOrDefault(x => x.Id == id);
        return p is null ? TypedResults.NotFound() : TypedResults.Ok(p);
    }

    private static Ok<ConstantsResponse> GetConstants(
        [FromQuery] string? locale,
        [FromQuery] int? schemaVersion)
    {
        _ = locale;
        _ = schemaVersion;
        return TypedResults.Ok(ConstantsResponse.Instance);
    }

    private static async Task<Ok<TrafficSimulationReport>> SimulateApiTrafficAsync(
        HttpRequest request,
        CancellationToken cancellationToken)
    {
        using var client = CreateLoopbackClient(request);
        var baseUri = $"{request.Scheme}://{request.Host}";
        var rnd = Random.Shared;
        var calls = new List<TrafficCallRecord>(60);

        for (var n = 0; n < 20; n++)
        {
            var cat = Categories[rnd.Next(Categories.Length)];
            var page = rnd.Next(1, 6);
            var size = rnd.Next(5, 26);
            var minP = rnd.Next(0, 80);
            var maxP = rnd.Next(minP + 1, 250);
            var sort = rnd.Next(0, 2) == 0 ? "price" : "name";
            var qs =
                $"category={Uri.EscapeDataString(cat)}&page={page}&pageSize={size}&minPrice={minP}&maxPrice={maxP}&sort={sort}";
            var url = $"{baseUri}/api/items?{qs}";
            var (code, ms) = await TimedGetAsync(client, url, cancellationToken).ConfigureAwait(false);
            calls.Add(new TrafficCallRecord("GET", "/api/items", code, ms, qs));
        }

        for (var n = 0; n < 20; n++)
        {
            var id = rnd.Next(1, Catalog.Length + 10);
            var extra = rnd.Next(0, 3) == 0 ? "?fields=minimal" : string.Empty;
            var path = $"/api/items/{id}{extra}";
            var url = $"{baseUri}{path}";
            var (code, ms) = await TimedGetAsync(client, url, cancellationToken).ConfigureAwait(false);
            calls.Add(new TrafficCallRecord("GET", path, code, ms, extra.TrimStart('?')));
        }

        var locales = new[] { "ru-RU", "en-US", "de-DE", "" };
        for (var n = 0; n < 20; n++)
        {
            var loc = locales[rnd.Next(locales.Length)];
            var v = rnd.Next(1, 4);
            var include = rnd.Next(0, 2) == 0 ? "&includeDeprecated=true" : string.Empty;
            var qs = string.IsNullOrEmpty(loc)
                ? $"v={v}{include}"
                : $"locale={Uri.EscapeDataString(loc)}&v={v}{include}";
            var url = $"{baseUri}/api/constants?{qs}";
            var (code, ms) = await TimedGetAsync(client, url, cancellationToken).ConfigureAwait(false);
            calls.Add(new TrafficCallRecord("GET", "/api/constants", code, ms, qs));
        }

        var report = new TrafficSimulationReport(
            calls.Count,
            calls.Count(c => c.StatusCode is >= 200 and < 300),
            calls.Count(c => c.StatusCode is 0 or >= 400),
            calls);
        return TypedResults.Ok(report);
    }

    private static HttpClient CreateLoopbackClient(HttpRequest request)
    {
        var handler = new HttpClientHandler();
        var host = request.Host.Host;
        if (host.Equals("localhost", StringComparison.OrdinalIgnoreCase) ||
            host.Equals("127.0.0.1", StringComparison.OrdinalIgnoreCase))
        {
            handler.ServerCertificateCustomValidationCallback = static (_, _, _, _) => true;
        }

        return new HttpClient(handler, disposeHandler: true)
        {
            Timeout = TimeSpan.FromSeconds(60),
        };
    }

    private static async Task<(int StatusCode, long ElapsedMs)> TimedGetAsync(
        HttpClient client,
        string url,
        CancellationToken cancellationToken)
    {
        var sw = System.Diagnostics.Stopwatch.StartNew();
        try
        {
            using var response = await client.GetAsync(new Uri(url), cancellationToken).ConfigureAwait(false);
            sw.Stop();
            return ((int)response.StatusCode, sw.ElapsedMilliseconds);
        }
        catch
        {
            sw.Stop();
            return (0, sw.ElapsedMilliseconds);
        }
    }

    private static ProductDto[] BuildCatalog()
    {
        var rng = new Random(42);
        var items = new List<ProductDto>(50);
        var id = 1;
        foreach (var cat in Categories)
        {
            for (var i = 0; i < 10; i++)
            {
                items.Add(new ProductDto(
                    id++,
                    $"{cat[..1].ToUpperInvariant()}{cat[1..]} item {i + 1}",
                    cat,
                    Math.Round((decimal)(rng.NextDouble() * 200 + 5), 2)));
            }
        }

        return items.ToArray();
    }

    private sealed record ProductDto(int Id, string Name, string Category, decimal Price);

    private sealed record PagedItemsResponse(
        IReadOnlyList<ProductDto> Items,
        int TotalCount,
        int Page,
        int PageSize,
        int TotalPages);

    private sealed record ConstantsResponse
    {
        public static ConstantsResponse Instance { get; } = new();

        public string ApiVersion { get; init; } = "1.0";
        public IReadOnlyList<string> SupportedCurrencies { get; init; } = ["RUB", "USD", "EUR"];
        public IReadOnlyList<string> OrderStatuses { get; init; } = ["draft", "paid", "shipped", "cancelled"];
        public int MaxPageSize { get; init; } = 50;
        public string DocsUrl { get; init; } = "/swagger";
    }

    private sealed record TrafficCallRecord(
        string Method,
        string PathTemplate,
        int StatusCode,
        long DurationMs,
        string QuerySample);

    private sealed record TrafficSimulationReport(
        int TotalCalls,
        int Succeeded,
        int ErrorsOrFailures,
        IReadOnlyList<TrafficCallRecord> Calls);
}
