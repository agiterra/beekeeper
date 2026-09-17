/// Fractional ranks for ordered, concurrently edited lists.
///
/// A faithful port of `crates/buzz-core/src/fractional_rank.rs`, pinned to
/// `conformance/project-todo-fold/fixtures/rank-vectors.json`. A rank is a
/// base-62 string that compares as plain code units (the alphabet is in
/// ASCII order), made of an integer part whose head letter encodes its
/// length (`a`+1 digit … `z`+26 for non-negative, `Z`+1 … `A`+26 for
/// negative) and an optional fraction that never ends in `0`. Between any
/// two ranks another can always be minted, so moving one item never
/// rewrites its neighbours.
///
/// The one rule stated so every implementation agrees bit for bit: the
/// midpoint digit between fraction digits `a` and `b` is `(a + b + 1) ~/ 2`.
library;

/// The digit alphabet, in ASCII order so code-unit comparison is numeric
/// order.
const rankDigits =
    '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz';

/// Maximum length of a rank. Inserting between the same two neighbours grows
/// the fraction by one digit per insertion, so this is the depth a client
/// can go before it must re-rank the list.
const maxRankLength = 64;

/// The first rank of an empty list.
const firstRank = 'a0';

final _digits = rankDigits.codeUnits;
const _zero = 0x30; // '0'
const _upperA = 0x41; // 'A'
const _upperZ = 0x5a; // 'Z'
const _lowerA = 0x61; // 'a'
const _lowerZ = 0x7a; // 'z'

int? _digitValue(int unit) {
  final index = _digits.indexOf(unit);
  return index < 0 ? null : index;
}

/// How many code units the integer part is, given its head letter.
int _integerLength(int head) {
  if (head >= _lowerA && head <= _lowerZ) return head - _lowerA + 2;
  if (head >= _upperA && head <= _upperZ) return _upperZ - head + 2;
  throw FormatException(
    'rank must start with a letter (got ${String.fromCharCode(head)})',
  );
}

/// Split a rank into `(integer, fraction)`, validating the integer length.
({List<int> integer, List<int> fraction}) _splitRank(List<int> rank) {
  if (rank.isEmpty) throw const FormatException('rank must not be empty');
  final length = _integerLength(rank.first);
  if (rank.length < length) {
    throw const FormatException(
      'rank integer part is shorter than its head declares',
    );
  }
  return (integer: rank.sublist(0, length), fraction: rank.sublist(length));
}

/// `A` followed by 26 zeros: the smallest integer part, reserved as the
/// floor below which nothing can be minted.
final List<int> _smallestInteger = [_upperA, ...List.filled(26, _zero)];

bool _sameUnits(List<int> a, List<int> b) {
  if (a.length != b.length) return false;
  for (var i = 0; i < a.length; i++) {
    if (a[i] != b[i]) return false;
  }
  return true;
}

/// Check that [rank] is a well-formed rank. Throws a [FormatException]
/// naming the problem otherwise.
void validateRank(String rank) {
  final units = rank.codeUnits;
  if (units.length > maxRankLength) {
    throw const FormatException('rank exceeds $maxRankLength characters');
  }
  final parts = _splitRank(units);
  for (final unit in [...parts.integer.skip(1), ...parts.fraction]) {
    if (_digitValue(unit) == null) {
      throw FormatException(
        'rank contains a character outside 0-9A-Za-z '
        '(${String.fromCharCode(unit)})',
      );
    }
  }
  if (parts.fraction.isNotEmpty && parts.fraction.last == _zero) {
    throw const FormatException('rank fraction must not end in 0');
  }
  if (_sameUnits(parts.integer, _smallestInteger) && parts.fraction.isEmpty) {
    throw const FormatException(
      'rank is below the smallest representable order',
    );
  }
}

/// `true` when [rank] passes [validateRank].
bool isValidRank(String rank) {
  try {
    validateRank(rank);
    return true;
  } on FormatException {
    return false;
  }
}

/// The string midpoint of fractions [a] (possibly empty, meaning zero) and
/// [b] (`null` meaning one). Precondition: `a < b`, neither ends in `0`.
List<int> _midpoint(List<int> a, List<int>? b) {
  if (b != null) {
    // Peel the longest common prefix; a missing digit of `a` reads as `0`,
    // which is what a shorter `a < b` means.
    var n = 0;
    while (n < b.length && (n < a.length ? a[n] : _zero) == b[n]) {
      n++;
    }
    if (n > 0) {
      return [
        ...b.sublist(0, n),
        ..._midpoint(a.sublist(n < a.length ? n : a.length), b.sublist(n)),
      ];
    }
  }
  final digitA = a.isEmpty ? 0 : (_digitValue(a.first) ?? 0);
  final digitB = b == null || b.isEmpty
      ? _digits.length
      : (_digitValue(b.first) ?? _digits.length);
  if (digitB - digitA > 1) {
    final mid = (digitA + digitB + 1) ~/ 2;
    return [_digits[mid]];
  }
  // The first digits are adjacent. If `b` has more digits, its first digit
  // alone already sits strictly between (`b`'s tail is positive because it
  // never ends in 0). Otherwise descend into `a`'s tail against "one".
  if (b != null && b.length > 1) return [b.first];
  return [
    _digits[digitA],
    ..._midpoint(a.length > 1 ? a.sublist(1) : [], null),
  ];
}

/// The next integer part after [x], or `null` at the top of the range.
List<int>? _incrementInteger(List<int> x) {
  final head = x.first;
  final digits = x.sublist(1);
  var carry = true;
  for (var i = digits.length - 1; i >= 0 && carry; i--) {
    final value = _digitValue(digits[i]);
    if (value == null) return null;
    if (value + 1 == _digits.length) {
      digits[i] = _zero;
    } else {
      digits[i] = _digits[value + 1];
      carry = false;
    }
  }
  if (!carry) return [head, ...digits];
  if (head == _upperZ) return firstRank.codeUnits.toList();
  if (head == _lowerZ) return null;
  final next = head + 1;
  if (next > _lowerA) {
    digits.add(_zero);
  } else {
    digits.removeLast();
  }
  return [next, ...digits];
}

/// The previous integer part before [x], or `null` at the bottom of the
/// range.
List<int>? _decrementInteger(List<int> x) {
  final head = x.first;
  final digits = x.sublist(1);
  final last = _digits.last;
  var borrow = true;
  for (var i = digits.length - 1; i >= 0 && borrow; i--) {
    final value = _digitValue(digits[i]);
    if (value == null) return null;
    if (value == 0) {
      digits[i] = last;
    } else {
      digits[i] = _digits[value - 1];
      borrow = false;
    }
  }
  if (!borrow) return [head, ...digits];
  if (head == _lowerA) return [_upperZ, last];
  if (head == _upperA) return null;
  final prev = head - 1;
  if (prev < _upperZ) {
    digits.add(last);
  } else {
    digits.removeLast();
  }
  return [prev, ...digits];
}

/// Mint a rank strictly between [after] and [before].
///
/// `null` on the left means "before everything" and `null` on the right
/// means "after everything"; `rankBetween(null, null)` is [firstRank]. Both
/// bounds must pass [validateRank] and `after < before` must hold, otherwise
/// a [FormatException] names the problem — a caller that computed its
/// neighbours from a stale fold gets told rather than handed a rank that
/// sorts somewhere else.
String rankBetween(String? after, String? before) {
  if (after != null) validateRank(after);
  if (before != null) validateRank(before);
  if (after != null && before != null && after.compareTo(before) >= 0) {
    throw FormatException('rank bounds are not ordered ($after >= $before)');
  }
  final List<int> minted;
  if (after == null && before == null) {
    minted = firstRank.codeUnits.toList();
  } else if (after == null) {
    final b = _splitRank(before!.codeUnits);
    if (_sameUnits(b.integer, _smallestInteger)) {
      minted = [...b.integer, ..._midpoint(const [], b.fraction)];
    } else if (b.fraction.isNotEmpty) {
      minted = b.integer;
    } else {
      final previous = _decrementInteger(b.integer);
      if (previous == null) {
        throw const FormatException('rank cannot go below the smallest order');
      }
      minted = previous;
    }
  } else if (before == null) {
    final a = _splitRank(after.codeUnits);
    minted =
        _incrementInteger(a.integer) ??
        [...a.integer, ..._midpoint(a.fraction, null)];
  } else {
    final a = _splitRank(after.codeUnits);
    final b = _splitRank(before.codeUnits);
    if (_sameUnits(a.integer, b.integer)) {
      minted = [...a.integer, ..._midpoint(a.fraction, b.fraction)];
    } else {
      final next = _incrementInteger(a.integer);
      if (next == null) {
        throw const FormatException('rank cannot go above the largest order');
      }
      if (String.fromCharCodes(next).compareTo(before) < 0) {
        minted = next;
      } else {
        minted = [...a.integer, ..._midpoint(a.fraction, null)];
      }
    }
  }
  if (minted.length > maxRankLength) {
    throw FormatException(
      'rank between $after and $before would exceed $maxRankLength '
      'characters; re-rank the list',
    );
  }
  return String.fromCharCodes(minted);
}
