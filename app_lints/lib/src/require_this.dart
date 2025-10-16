import 'package:analyzer/dart/ast/ast.dart'
    show
        AstNode,
        ConstructorDeclaration,
        ConstructorInitializer,
        DefaultFormalParameter,
        FieldDeclaration,
        MethodDeclaration,
        SimpleIdentifier,
        VariableDeclaration,
        VariableDeclarationList;
import 'package:analyzer/dart/element/element.dart'
    show
        ConstructorElement,
        Element,
        ExecutableElement,
        ExtensionElement,
        ExtensionTypeElement,
        InterfaceElement,
        PropertyAccessorElement,
        PropertyInducingElement;
import 'package:analyzer/error/listener.dart' show DiagnosticReporter;
import 'package:custom_lint_builder/custom_lint_builder.dart'
    show DartLintRule, LintCode, CustomLintContext, CustomLintResolver;

class RequireThis extends DartLintRule {
  const RequireThis() : super(code: _code);

  static const LintCode _code = LintCode(
    name: 'require_this',
    problemMessage: 'Use `this.{0}`',
  );

  @override
  void run(
    CustomLintResolver resolver,
    DiagnosticReporter reporter,
    CustomLintContext context,
  ) {
    context.registry.addSimpleIdentifier((node) {
      if (!_shouldReport(node)) {
        return;
      }

      final displayName = node.name;
      reporter.atNode(node, code, arguments: <Object>[displayName]);
    });
  }

  static bool _shouldReport(SimpleIdentifier node) {
    if (node.inDeclarationContext()) return false;
    if (node.isQualified) return false;
    if (!_isWithinInstanceMemberBody(node)) return false;
    if (_isInDisallowedInitializer(node)) return false;

    final element = node.element;
    if (element == null) return false;
    return _isImplicitInstanceMember(element);
  }

  static bool _isWithinInstanceMemberBody(SimpleIdentifier node) {
    for (
      AstNode? ancestor = node.parent;
      ancestor != null;
      ancestor = ancestor.parent
    ) {
      if (ancestor is MethodDeclaration) {
        final body = ancestor.body;
        if (ancestor.isStatic) return false;
        return _isNodeWithin(body, node);
      }

      if (ancestor is ConstructorDeclaration) {
        final body = ancestor.body;
        if (ancestor.factoryKeyword != null) return false;
        return _isNodeWithin(body, node);
      }

      if (ancestor is FieldDeclaration) {
        return false;
      }
    }
    return false;
  }

  static bool _isInDisallowedInitializer(SimpleIdentifier node) {
    final constructorInitializer = node
        .thisOrAncestorOfType<ConstructorInitializer>();
    if (constructorInitializer != null) {
      return _isNodeWithin(constructorInitializer, node);
    }

    final defaultParameter = node
        .thisOrAncestorOfType<DefaultFormalParameter>();
    if (defaultParameter != null) {
      final defaultValue = defaultParameter.defaultValue;
      if (defaultValue != null && _isNodeWithin(defaultValue, node)) {
        return true;
      }
    }

    final variable = node.thisOrAncestorOfType<VariableDeclaration>();
    if (variable != null) {
      final initializer = variable.initializer;
      final variableList = variable.parent;
      if (initializer != null &&
          _isNodeWithin(initializer, node) &&
          variableList is VariableDeclarationList &&
          variableList.parent is FieldDeclaration) {
        return true;
      }
    }

    return false;
  }

  static bool _isImplicitInstanceMember(Element element) {
    Element current = element.baseElement;

    if (current is PropertyAccessorElement) {
      if (current.isStatic) return false;
      current = current.variable;
    }

    if (current is ExecutableElement) {
      if (current is ConstructorElement) return false;
      if (current.isStatic) return false;
    } else if (current is PropertyInducingElement) {
      if (current.isStatic) return false;
    } else {
      return false;
    }

    final enclosing = current.enclosingElement;
    return enclosing is InterfaceElement ||
        enclosing is ExtensionElement ||
        enclosing is ExtensionTypeElement;
  }

  static bool _isNodeWithin(AstNode? scope, AstNode node) {
    if (scope == null) return false;
    return node.offset >= scope.offset && node.end <= scope.end;
  }
}
