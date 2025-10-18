/// Custom flutter/dart lint rules for Lexe app.
library;

import 'package:custom_lint_builder/custom_lint_builder.dart'
    show CustomLintConfigs, LintRule, PluginBase;

import 'src/require_this.dart' show RequireThis;

PluginBase createPlugin() => _AppLintsPlugin();

class _AppLintsPlugin extends PluginBase {
  @override
  List<LintRule> getLintRules(CustomLintConfigs configs) => const <LintRule>[
    RequireThis(),
  ];
}
