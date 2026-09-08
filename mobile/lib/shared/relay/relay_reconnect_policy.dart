/// Spreads a reconnect delay across `[0.75, 1.25) × baseMs` so that several
/// devices sharing one key (and one relay restart) do not all replay their
/// subscriptions in the same instant. [random] is a uniform draw in `[0, 1)`.
int jitteredReconnectDelayMs(int baseMs, double random) {
  assert(random >= 0 && random < 1, 'random must be a uniform draw in [0, 1)');
  return (baseMs * (0.75 + 0.5 * random)).round();
}
