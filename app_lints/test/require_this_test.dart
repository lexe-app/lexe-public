import 'dart:io';

import 'package:analyzer/dart/analysis/results.dart';
import 'package:analyzer/dart/analysis/utilities.dart';
import 'package:analyzer/diagnostic/diagnostic.dart';
import 'package:app_lints/src/require_this.dart';
import 'package:test/test.dart';

Future<List<Diagnostic>> lint(String code) async {
  final result = await _resolve(code);
  final diagnostics = await const RequireThis().testRun(result);
  return diagnostics.cast<Diagnostic>();
}

Future<ResolvedUnitResult> _resolve(String code) async {
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
}
