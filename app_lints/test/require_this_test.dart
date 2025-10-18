import 'dart:io' show Directory, File;

import 'package:analyzer/dart/analysis/results.dart' show ResolvedUnitResult;
import 'package:analyzer/dart/analysis/utilities.dart' show resolveFile;
import 'package:analyzer/diagnostic/diagnostic.dart' show Diagnostic;
import 'package:app_lints/src/require_this.dart' show RequireThis;
import 'package:analyzer_plugin/protocol/protocol_common.dart' show SourceEdit;
import 'package:custom_lint_builder/custom_lint_builder.dart' show DartFix;
import 'package:test/test.dart';

void main() {
  group('RequireThis', () {
    test('reports unqualified instance member reads', () async {
      const code = '''
        class Counter {
          Counter(this.value);

          final int value;

          int increment() {
            return value;
          }
        }
      ''';
      final diagnostics = await lint(code);
      expect(diagnostics, hasLength(1));
      final diagnostic = diagnostics.single;
      expect(diagnostic.diagnosticCode.name, 'require_this');
      expect(
        diagnostic.offset,
        code.indexOf('return value;') + 'return '.length,
      );
    });

    test('ignores already qualified references', () async {
      final diagnostics = await lint('''
        class Counter {
          Counter(this.value);

          final int value;

          int increment() {
            return this.value;
          }
        }
      ''');
      expect(diagnostics, isEmpty);
    });

    test('ignores contexts where `this` cannot be used', () async {
      final diagnostics = await lint('''
        class Example {
          Example()
              : assigned = 0,
                copy = assigned,
                doubled = value * 2;

          final int value = 1;
          final int assigned;
          final int copy;
          final int doubled;

          final int fieldInitializer = value;

          void method([int initial = value]) {}

          static const int staticValue = 1;

          static int readStatic() => staticValue;
        }
      ''');
      expect(diagnostics, isEmpty);
    });

    test('allows local variable shadowing a member', () async {
      final diagnostics = await lint('''
        class Counter {
          Counter(this.value);

          final int value;

          void logValue() {
            final value = 42;
            print(value);
          }
        }
      ''');
      expect(diagnostics, isEmpty);
    });
  });

  group('RequireThis quick-fix', () {
    test('prefixes instance member reads with `this.`', () async {
      await assertFixes(
        '''
          class Counter {
            Counter(this.a, this.b);

            final int a;
            final int b;

            int sum() => a + b;
          }
        ''',
        '''
          class Counter {
            Counter(this.a, this.b);

            final int a;
            final int b;

            int sum() => this.a + this.b;
          }
        ''',
      );
    });
  });
}

Future<List<Diagnostic>> lint(String code) async {
  final result = await resolve(code);
  final diagnostics = await const RequireThis().testRun(result);
  return diagnostics.cast<Diagnostic>();
}

Future<void> assertFixes(String source, String expected) async {
  final result = await resolve(source);
  const lint = RequireThis();
  final diagnostics = await lint.testRun(result);

  expect(diagnostics, isNotEmpty, reason: 'Expected at least one diagnostic.');

  final fix = lint.getFixes().single as DartFix;
  final edits = <SourceEdit>[];
  for (final diagnostic in diagnostics.cast<Diagnostic>()) {
    final changes = await fix.testRun(result, diagnostic, diagnostics);

    expect(changes, hasLength(1));
    final change = changes.single.change;

    expect(change.message, 'Prefix with `this.`');
    expect(change.edits, hasLength(1));
    expect(change.edits.single.edits, hasLength(1));

    final edit = change.edits.single.edits.single;
    edits.add(edit);
  }
  final updated = applyEdits(source, edits);
  expect(updated, expected);
}

Future<ResolvedUnitResult> resolve(String code) async {
  final tempDir = await Directory.systemTemp.createTemp('require_this_test');
  try {
    final file = File('${tempDir.path}/test.dart');
    await file.writeAsString(code);
    final result = await resolveFile(path: file.path);
    return result as ResolvedUnitResult;
  } finally {
    await tempDir.delete(recursive: true);
  }
}

String applyEdits(String source, List<SourceEdit> edits) {
  var updated = source;
  final sortedEdits = edits.toList()
    ..sort((a, b) => b.offset.compareTo(a.offset));
  for (final edit in sortedEdits) {
    updated = updated.replaceRange(
      edit.offset,
      edit.offset + edit.length,
      edit.replacement,
    );
  }
  return updated;
}
