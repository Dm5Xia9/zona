using Microsoft.AspNetCore.Builder;
using Microsoft.Extensions.DependencyInjection;

namespace Zona.ProxyLib;

public static class ZonaProxyExtensions
{
    public static IServiceCollection AddZona(this IServiceCollection services, Action<ZonaOptions>? configure = null)
    {
        services.AddOptions<ZonaOptions>().BindConfiguration(ZonaOptions.SectionName);
        if (configure is not null)
            services.Configure(configure);

        services.AddSingleton<ZonaTeachQueue>();
        services.AddSingleton<IZonaTeachQueue>(sp => sp.GetRequiredService<ZonaTeachQueue>());
        services.AddSingleton<ZonaTeachSendGate>();
        services.AddSingleton<IZonaTeachSendGate>(sp => sp.GetRequiredService<ZonaTeachSendGate>());
        services.AddHostedService<ZonaProcessHostedService>();
        services.AddHostedService<ZonaTrainingStatusPoller>();
        services.AddHostedService<ZonaTeachDispatchWorker>();

        services.AddHttpClient(ZonaHttpNames.Client, (sp, c) =>
        {
            var o = sp.GetRequiredService<Microsoft.Extensions.Options.IOptions<ZonaOptions>>().Value;
            c.BaseAddress = ZonaOptions.BuildBaseUri(o.Listen);
            c.Timeout = TimeSpan.FromSeconds(3);
        });

        return services;
    }

    /// <summary>
    /// После <c>UseRouting()</c>: по метаданным сопоставлённого эндпоинта решает, слать ли teach;
    /// после выполнения конвейера ставит сигнал в очередь (длительность, статус, TTFB).
    /// </summary>
    public static IApplicationBuilder UseZonaTeach(this IApplicationBuilder app) =>
        app.UseMiddleware<ZonaTeachMiddleware>();
}
