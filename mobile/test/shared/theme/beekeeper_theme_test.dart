import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:beekeeper/shared/theme/theme.dart';
import 'package:beekeeper/shared/widgets/frosted_app_bar.dart';

void main() {
  group('Beekeeper theme catalog entries', () {
    test('both halves are in the catalog', () {
      expect(findTheme(beekeeperThemeName), isNotNull);
      expect(findTheme(beekeeperDarkThemeName), isNotNull);
    });

    test('borrow the GitHub palettes', () {
      final beekeeper = findTheme(beekeeperThemeName)!;
      final github = findTheme('github-light')!;
      expect(beekeeper.bg, github.bg);
      expect(beekeeper.fg, github.fg);
      expect(beekeeper.comment, github.comment);

      final beekeeperDark = findTheme(beekeeperDarkThemeName)!;
      final githubDark = findTheme('github-dark')!;
      expect(beekeeperDark.bg, githubDark.bg);
      expect(beekeeperDark.fg, githubDark.fg);
      expect(beekeeperDark.comment, githubDark.comment);
    });

    test('are a light/dark pair', () {
      expect(findTheme(beekeeperThemeName)!.isDark, isFalse);
      expect(findTheme(beekeeperDarkThemeName)!.isDark, isTrue);
      expect(themePairFor(beekeeperThemeName), beekeeperDarkThemeName);
      expect(themePairFor(beekeeperDarkThemeName), beekeeperThemeName);
    });

    test('appear as a single System-mode option labelled "Beekeeper"', () {
      final paired = themeGroups().paired.map((t) => t.name);
      expect(paired, contains(beekeeperThemeName));
      expect(paired, isNot(contains(beekeeperDarkThemeName)));
      expect(pairedThemeLabel(beekeeperThemeName), 'Beekeeper');
      expect(
        themeSelectionLabel(beekeeperThemeName, ThemeMode.system),
        'Beekeeper',
      );
      expect(
        themeSelectionLabel(beekeeperDarkThemeName, ThemeMode.system),
        'Beekeeper',
      );
    });

    test('forces neutral rendering without changing the stored accent', () {
      const storedAccent = '#ef4444';

      expect(
        effectiveAccentIndex(beekeeperThemeName, storedAccent),
        neutralAccentIndex,
      );
      expect(
        effectiveAccentIndex(beekeeperDarkThemeName, storedAccent),
        neutralAccentIndex,
      );
      expect(
        effectiveAccentIndex('github-light', storedAccent),
        accentIndexForWireValue(storedAccent),
      );
      expect(storedAccent, '#ef4444');
    });

    test('resolve across brightnesses like any other pair', () {
      final resolved = resolveSchemes(beekeeperThemeName, ThemeMode.system);
      expect(resolved.forcedMode, isNull);
      expect(resolved.light.brightness, Brightness.light);
      expect(resolved.dark.brightness, Brightness.dark);
      expect(resolved.lightTheme?.name, beekeeperThemeName);
      expect(resolved.darkTheme?.name, beekeeperDarkThemeName);

      expect(
        effectiveTheme(beekeeperThemeName, ThemeMode.dark)?.name,
        beekeeperDarkThemeName,
      );
      expect(
        effectiveTheme(beekeeperDarkThemeName, ThemeMode.light)?.name,
        beekeeperThemeName,
      );
    });

    test(
      'fallbacks expose the effective Beekeeper theme for gradient selection',
      () {
        final coerced = resolveSchemes('nord', ThemeMode.light);
        expect(coerced.lightTheme?.name, beekeeperThemeName);
        expect(
          beekeeperTopSectionGradient(
            coerced.lightTheme!.name,
            coerced.light.brightness,
          ),
          isNotNull,
        );

        final unknown = resolveSchemes('not-a-theme', ThemeMode.light);
        expect(unknown.lightTheme?.name, beekeeperThemeName);
        expect(
          beekeeperTopSectionGradient(
            unknown.lightTheme!.name,
            unknown.light.brightness,
          ),
          isNotNull,
        );
      },
    );
  });

  group('beekeeperTopSectionGradient', () {
    test('is null for non-Beekeeper themes', () {
      expect(
        beekeeperTopSectionGradient('github-light', Brightness.light),
        isNull,
      );
      expect(beekeeperTopSectionGradient('nord', Brightness.dark), isNull);
    });

    test('paints top to bottom for both halves of the pair', () {
      for (final name in [beekeeperThemeName, beekeeperDarkThemeName]) {
        final gradient = beekeeperTopSectionGradient(name, Brightness.light);
        expect(gradient, isNotNull, reason: '$name should be gradient-backed');
        expect(gradient!.begin, Alignment.topCenter);
        expect(gradient.end, Alignment.bottomCenter);
        expect(gradient.colors, hasLength(2));
      }
    });

    test('brightness selects the stops, not the theme name', () {
      // Both halves enable the gradient, so System mode keeps it on across an
      // OS switch — the applied brightness alone decides which stops are used.
      final light = beekeeperTopSectionGradient(
        beekeeperThemeName,
        Brightness.light,
      )!;
      final dark = beekeeperTopSectionGradient(
        beekeeperThemeName,
        Brightness.dark,
      )!;

      expect(light.colors, isNot(dark.colors));
      expect(
        beekeeperTopSectionGradient(
          beekeeperDarkThemeName,
          Brightness.dark,
        )!.colors,
        dark.colors,
      );
      expect(
        beekeeperTopSectionGradient(
          beekeeperDarkThemeName,
          Brightness.light,
        )!.colors,
        light.colors,
      );
    });

    test('is opaque so the color replaces the frosted fill', () {
      for (final brightness in Brightness.values) {
        final gradient = beekeeperTopSectionGradient(
          beekeeperThemeName,
          brightness,
        )!;
        for (final color in gradient.colors) {
          expect(color.a, 1.0);
        }
      }
    });
  });

  group('theme threading', () {
    BoxDecoration barDecoration(WidgetTester tester) {
      final container = tester
          .widgetList<Container>(
            find.descendant(
              of: find.byType(FrostedAppBar),
              matching: find.byType(Container),
            ),
          )
          .first;
      return container.decoration! as BoxDecoration;
    }

    Widget harness(ThemeData theme) => MaterialApp(
      theme: theme,
      home: Builder(
        builder: (context) => Stack(
          children: [
            FrostedAppBar(
              gradient: context.appColors.topSectionGradient,
              title: const Text('Home'),
            ),
          ],
        ),
      ),
    );

    testWidgets('AppTheme carries the gradient to the top section', (
      tester,
    ) async {
      await tester.pumpWidget(
        harness(
          AppTheme.light(
            topSectionGradient: beekeeperTopSectionGradient(
              beekeeperThemeName,
              Brightness.light,
            ),
          ),
        ),
      );

      final decoration = barDecoration(tester);
      expect(decoration.gradient, isNotNull);
      // A BoxDecoration cannot paint a color and a gradient at once.
      expect(decoration.color, isNull);
    });

    testWidgets('non-Beekeeper themes keep the frosted surface fill', (
      tester,
    ) async {
      await tester.pumpWidget(harness(AppTheme.light()));

      final decoration = barDecoration(tester);
      expect(decoration.gradient, isNull);
      expect(decoration.color, isNotNull);
    });

    testWidgets('Beekeeper section labels use 80% neutral foreground', (
      tester,
    ) async {
      await tester.pumpWidget(
        harness(
          AppTheme.light(
            topSectionGradient: beekeeperTopSectionGradient(
              beekeeperThemeName,
              Brightness.light,
            ),
          ),
        ),
      );

      final context = tester.element(find.text('Home'));
      expect(
        navigationSectionForeground(context),
        Colors.black.withValues(alpha: 0.8),
      );
    });

    testWidgets('navigation roles inherit non-Beekeeper theme tokens', (
      tester,
    ) async {
      const primaryForeground = Color(0xFF123456);
      const secondaryForeground = Color(0xFF789ABC);
      const searchSurface = Color(0xFFDEF012);
      final theme = ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.purple).copyWith(
          onSurface: primaryForeground,
          onSurfaceVariant: secondaryForeground,
          surfaceContainerHighest: searchSurface,
        ),
      );

      await tester.pumpWidget(
        MaterialApp(
          theme: theme,
          home: const Scaffold(body: SizedBox()),
        ),
      );

      final context = tester.element(find.byType(SizedBox));
      expect(navigationPrimaryForeground(context), primaryForeground);
      expect(navigationSecondaryForeground(context), secondaryForeground);
      expect(navigationSectionForeground(context), secondaryForeground);
      expect(navigationSearchSurface(context), searchSurface);
      expect(
        navigationDivider(context, 0.15),
        primaryForeground.withValues(alpha: 0.15),
      );
    });
  });

  group('isBeekeeperTheme', () {
    test('matches only the Beekeeper pair', () {
      expect(isBeekeeperTheme(beekeeperThemeName), isTrue);
      expect(isBeekeeperTheme(beekeeperDarkThemeName), isTrue);
      expect(isBeekeeperTheme('github-light'), isFalse);
      expect(isBeekeeperTheme(''), isFalse);
    });
  });
}
