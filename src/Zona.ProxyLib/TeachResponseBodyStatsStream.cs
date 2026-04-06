namespace Zona.ProxyLib;

/// <summary>Подсчёт паттерна байт по мере записи ответа, без буферизации всего тела.</summary>
internal sealed class TeachResponseBodyStatsStream : Stream
{
    private readonly Stream _inner;
    private readonly int _maxStatsBytes;
    private int _sampled;
    /// <summary>Все байты, прошедшие через поток (для teach, если нет Content-Length).</summary>
    private long _totalBytesWritten;
    private readonly int[] _hist = new int[256];
    private long _oneBits;
    private bool _computed;
    private TrafficPatternStats? _stats;

    public TeachResponseBodyStatsStream(Stream inner, int maxStatsBytes)
    {
        _inner = inner;
        _maxStatsBytes = Math.Max(0, maxStatsBytes);
    }

    public TrafficPatternStats? GetCapturedStats()
    {
        if (!_computed)
        {
            _stats = ZonaTeachBodyStats.FromHistogram(_hist, _sampled, _oneBits);
            _computed = true;
        }
        return _stats;
    }

    /// <summary>Сколько байт реально записано в ответ (включая за пределами префикса для гистограммы).</summary>
    public long TotalBytesWritten => _totalBytesWritten;

    public override bool CanRead => false;
    public override bool CanSeek => false;
    public override bool CanWrite => _inner.CanWrite;
    public override long Length => throw new NotSupportedException();
    public override long Position
    {
        get => throw new NotSupportedException();
        set => throw new NotSupportedException();
    }

    public override void Flush() => _inner.Flush();

    public override int Read(byte[] buffer, int offset, int count) => throw new NotSupportedException();

    public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();

    public override void SetLength(long value) => throw new NotSupportedException();

    public override void Write(byte[] buffer, int offset, int count) =>
        Write(buffer.AsSpan(offset, count));

    public override void Write(ReadOnlySpan<byte> buffer)
    {
        _totalBytesWritten += buffer.Length;
        if (_sampled < _maxStatsBytes && buffer.Length > 0)
        {
            int take = Math.Min(buffer.Length, _maxStatsBytes - _sampled);
            if (take > 0)
            {
                ZonaTeachBodyStats.AddChunk(buffer[..take], _hist, ref _oneBits);
                _sampled += take;
            }
        }
        _inner.Write(buffer);
    }

    public override async Task WriteAsync(byte[] buffer, int offset, int count, CancellationToken cancellationToken) =>
        await WriteAsync(buffer.AsMemory(offset, count), cancellationToken).ConfigureAwait(false);

    public override async ValueTask WriteAsync(ReadOnlyMemory<byte> buffer, CancellationToken cancellationToken = default)
    {
        _totalBytesWritten += buffer.Length;
        if (_sampled < _maxStatsBytes && buffer.Length > 0)
        {
            int take = Math.Min(buffer.Length, _maxStatsBytes - _sampled);
            if (take > 0)
            {
                ZonaTeachBodyStats.AddChunk(buffer.Span[..take], _hist, ref _oneBits);
                _sampled += take;
            }
        }
        await _inner.WriteAsync(buffer, cancellationToken).ConfigureAwait(false);
    }

    public override async Task FlushAsync(CancellationToken cancellationToken) =>
        await _inner.FlushAsync(cancellationToken).ConfigureAwait(false);
}
