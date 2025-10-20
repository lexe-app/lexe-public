/// Custom flutter/dart lint rules for Lexe app.
library;

import 'package:app_lints/src/require_this.dart' show RequireThis;
import 'package:custom_lint_builder/custom_lint_builder.dart'
    show CustomLintConfigs, LintRule, PluginBase;

PluginBase createPlugin() => _AppLintsPlugin();

class _AppLintsPlugin extends PluginBase {
  @override
  List<LintRule> getLintRules(CustomLintConfigs configs) => const <LintRule>[
    RequireThis(),
  ];
}
