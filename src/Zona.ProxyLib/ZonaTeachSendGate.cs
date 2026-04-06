namespace Zona.ProxyLib;

/// <summary>Пока Zona сообщает, что обучение не завершено — можно слать teach; после <c>trainingComplete</c> — нет.</summary>
public interface IZonaTeachSendGate
{
    bool ShouldSendTeach { get; }
}

public sealed class ZonaTeachSendGate : IZonaTeachSendGate
{
    private int _halt;

    public bool ShouldSendTeach => System.Threading.Volatile.Read(ref _halt) == 0;

    internal void SetTrainingComplete() => System.Threading.Volatile.Write(ref _halt, 1);
}
