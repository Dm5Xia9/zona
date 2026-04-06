using Zona.Api;
using Zona.ProxyLib;

var builder = WebApplication.CreateBuilder(args);

builder.Services.AddEndpointsApiExplorer();
builder.Services.AddSwaggerGen();
builder.Services.AddZona();

var app = builder.Build();

app.UseHttpsRedirection();

app.UseRouting();
app.UseZonaTeach();

if (app.Environment.IsDevelopment())
{
    app.UseSwagger();
    app.UseSwaggerUI();
}

app.MapZonaDevelopmentStatus();
app.MapDemoApi();

app.Run();
