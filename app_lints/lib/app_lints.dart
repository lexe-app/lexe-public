import 'package:custom_lint_builder/custom_lint_builder.dart';

PluginBase createPlugin() => _AppLintsPlugin();

class _AppLintsPlugin extends PluginBase {
  @override
  List<LintRule> getLintRules(CustomLintConfigs configs) => const <LintRule>[];
}
