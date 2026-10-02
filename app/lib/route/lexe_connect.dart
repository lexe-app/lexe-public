/// LexeConnect: approve or reject another app's request for wallet
/// credentials.
library;

import 'dart:async' show unawaited;
import 'dart:convert' show utf8;

import 'package:app_rs_dart/ffi/app.dart' show AppHandle;
import 'package:app_rs_dart/ffi/types.dart'
    show
        CredentialDecision,
        CredentialDecision_Approve,
        CredentialDecision_Reject,
        CredentialRequest,
        RequesterDisplay,
        RequesterDisplay_Unverified,
        RequesterDisplay_Verified,
        Scope;
import 'package:flutter/material.dart';
import 'package:lexeapp/components.dart'
    show
        AnimatedFillButton,
        EditableInfoRow,
        ErrorMessage,
        ErrorMessageSection,
        HeadingText,
        InfoCard,
        InfoRow,
        LxBackButton,
        LxCloseButton,
        LxCloseButtonKind,
        LxOutlinedButton,
        ScrollableSinglePageBody;
import 'package:lexeapp/date_time_ext.dart';
import 'package:lexeapp/prelude.dart';
import 'package:lexeapp/route/clients.dart'
    show ExpirationRow, ScopeExt, formatExpiration;
import 'package:lexeapp/string_ext.dart';
import 'package:lexeapp/style.dart'
    show Fonts, LxColors, LxIcons, LxRadius, Space;
import 'package:lexeapp/url.dart' as url;

/// The user's delivered decision on a [CredentialRequest].
enum LexeConnectFlowResult { approved, rejected }

/// The LexeConnect approval screen. Pops with a [LexeConnectFlowResult] once
/// the response is delivered, or with null if the user backs out.
class LexeConnectPage extends StatefulWidget {
  const LexeConnectPage({super.key, required this.app, required this.request});

  final AppHandle app;
  final CredentialRequest request;

  @override
  State<LexeConnectPage> createState() => _LexeConnectPageState();
}

class _LexeConnectPageState extends State<LexeConnectPage> {
  late final TextEditingController labelController = TextEditingController(
    text: this.widget.request.label ?? this.defaultLabel(),
  );
  final FocusNode labelFocusNode = FocusNode();

  /// The credential's expiration in ms since the UNIX epoch, or null to never
  /// expire. Prefilled from the request, else one year out.
  late final ValueNotifier<int?> expiresAtMs = ValueNotifier(
    // TODO(max): Once budgets are supported, default spending credentials
    // with a budget attached to never expire.
    this.widget.request.expiresAt ??
        DateTime.now().addMonths(12).millisecondsSinceEpoch,
  );

  /// The decision being delivered, if any.
  final ValueNotifier<CredentialDecision?> pendingDecision = ValueNotifier(
    null,
  );
  final ValueNotifier<ErrorMessage?> errorMessage = ValueNotifier(null);

  late final List<AccessItem> accessItems = this.buildAccessItems();

  /// Indices of the collapsed [accessItems].
  final ValueNotifier<Set<int>> collapsedItems = ValueNotifier(const {});

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback(
      (_) => this.collapseItemsIfOverflowing(),
    );
  }

  @override
  void dispose() {
    this.collapsedItems.dispose();
    this.errorMessage.dispose();
    this.expiresAtMs.dispose();
    this.labelController.dispose();
    this.pendingDecision.dispose();
    this.labelFocusNode.dispose();
    super.dispose();
  }

  /// The label prefill when the request sets none. Uses a verified
  /// requester's name, else its domain, if it fits in a label.
  String? defaultLabel() {
    final label = switch (this.widget.request.requester) {
      RequesterDisplay_Verified(:final branding?) => branding.name,
      RequesterDisplay_Verified(:final domain) => domain,
      RequesterDisplay_Unverified() => null,
    };
    return (label != null && utf8.encode(label).length <= 64) ? label : null;
  }

  /// The requested scopes the app knows how to display, in canonical order.
  List<Scope> knownScopes() {
    final known = {
      for (final id in this.widget.request.scopes)
        if (Scope.fromStringId(s: id) case final scope?) scope,
    };
    return Scope.values.where(known.contains).toList();
  }

  /// [knownScopes], minus those another requested scope already covers,
  /// e.g. `full` alone stands in for every scope.
  List<Scope> displayedScopes() {
    final known = this.knownScopes();
    final covered = {for (final scope in known) ...scope.children()};
    return known.where((scope) => !covered.contains(scope)).toList();
  }

  /// Requested scopes this version of the app doesn't know.
  List<String> unknownScopes() => [
    for (final id in this.widget.request.scopes)
      if (Scope.fromStringId(s: id) == null) id,
  ];

  /// The rows of the access card. Unrecognized scopes block approval, so
  /// if any were requested, the first is the only row.
  List<AccessItem> buildAccessItems() {
    if (this.unknownScopes().firstOrNull case final id?) {
      return [
        (
          icon: LxIcons.warning,
          // The id is chosen by the requester, so quote it to keep it from
          // passing as Lexe's own text.
          title: Text.rich(
            TextSpan(
              children: [
                const TextSpan(text: "Unrecognized scope: "),
                TextSpan(text: '"$id"', style: _monoStyle),
              ],
            ),
          ),
          description: const Text(
            "Unrecognized scopes can't be approved. This scope is either "
            "invalid or not yet supported by this version of Lexe. "
            "Updating Lexe may resolve this.",
          ),
          kind: AccessKind.error,
        ),
      ];
    }

    return [
      for (final scope in this.displayedScopes())
        (
          icon: scope.icon(),
          title: Text(scope.title()),
          description: Text(scope.description()),
          kind: AccessKind.scope,
        ),
      if (this.widget.request.permissions case final permissions
          when permissions.isNotEmpty)
        (
          icon: LxIcons.sdk,
          title: const Text("API permissions"),
          description: Text(
            (permissions.toList()..sort()).join("\n"),
            style: _monoStyle,
          ),
          kind: AccessKind.permissions,
        ),
    ];
  }

  /// Collapses the scope rows if the fully expanded page has to scroll. The
  /// API permissions row stays expanded, so they can't be skimmed past.
  void collapseItemsIfOverflowing() {
    if (!this.mounted) return;
    final controller = PrimaryScrollController.maybeOf(this.context);
    final overflows =
        controller != null &&
        controller.positions.any((position) => position.maxScrollExtent > 0);
    if (overflows) {
      this.collapsedItems.value = {
        for (final (index, item) in this.accessItems.indexed)
          if (item.kind == AccessKind.scope) index,
      };
    }
  }

  void toggleItem(int index) {
    final collapsed = {...this.collapsedItems.value};
    if (!collapsed.remove(index)) collapsed.add(index);
    this.collapsedItems.value = collapsed;
  }

  bool canSpend() => this.knownScopes().any(
    (scope) => scope == Scope.spend || scope == Scope.full,
  );

  Future<void> onApprove() => this.respond(
    CredentialDecision.approve(
      label: this.labelController.text.trim().nonEmpty(),
      expiresAt: this.expiresAtMs.value,
    ),
  );

  Future<void> onReject() => this.respond(const CredentialDecision.reject());

  Future<void> respond(CredentialDecision decision) async {
    if (this.pendingDecision.value != null) return;

    this.pendingDecision.value = decision;
    this.errorMessage.value = null;

    final result = await Result.tryFfiAsync(
      () => this.widget.app.respondCredentialRequest(
        connectionString: this.widget.request.connectionString,
        decision: decision,
      ),
    );
    if (!this.mounted) return;

    final String? redirectUri;
    switch (result) {
      case Ok(:final ok):
        redirectUri = ok;
      case Err(:final err):
        error("LexeConnectPage: error responding: ${err.message}");
        this.pendingDecision.value = null;
        this.errorMessage.value = ErrorMessage(
          title: "Error responding to request",
          message: err.message,
        );
        return;
    }

    // A `redirect_uri` response is delivered by opening it in the requester.
    if (redirectUri != null) {
      final opened = await url.open(redirectUri);
      if (!this.mounted) return;
      if (opened.ok != true) {
        this.pendingDecision.value = null;
        this.errorMessage.value = const ErrorMessage(
          title: "Couldn't open the requesting app",
          message: "The response was not delivered.",
        );
        return;
      }
    }

    final flowResult = switch (decision) {
      CredentialDecision_Approve() => LexeConnectFlowResult.approved,
      CredentialDecision_Reject() => LexeConnectFlowResult.rejected,
    };
    info("LexeConnectPage: delivered: $flowResult");
    unawaited(Navigator.of(this.context).maybePop(flowResult));
  }

  @override
  Widget build(BuildContext context) {
    final request = this.widget.request;
    final canApprove = this.unknownScopes().isEmpty;

    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
        actions: const [
          LxCloseButton(kind: LxCloseButtonKind.closeFromRoot),
          SizedBox(width: Space.appBarTrailingPadding),
        ],
      ),
      body: ScrollableSinglePageBody(
        body: [
          RequesterHeader(requester: request.requester),
          const SizedBox(height: Space.s400),

          if (canApprove && this.canSpend()) const SpendWarningCard(),

          // Access
          InfoCard(
            header: const Text("Access"),
            bodyPadding: Space.s400,
            children: [
              ValueListenableBuilder(
                valueListenable: this.collapsedItems,
                builder: (context, collapsed, _) => Column(
                  children: [
                    for (final (index, item) in this.accessItems.indexed)
                      AccessRow(
                        item: item,
                        // Error rows always show why.
                        expanded:
                            item.kind == AccessKind.error ||
                            !collapsed.contains(index),
                        onTap: (item.kind == AccessKind.error)
                            ? null
                            : () => this.toggleItem(index),
                      ),
                  ],
                ),
              ),
            ],
          ),

          // Details, read-only if the request can't be approved
          InfoCard(
            header: const Text("Details"),
            bodyPadding: Space.s400,
            children: [
              if (request.account case final account?)
                InfoRow(
                  label: "Account",
                  value: account,
                  bodyPadding: Space.s400,
                ),
              if (canApprove)
                ValueListenableBuilder(
                  valueListenable: this.pendingDecision,
                  builder: (context, pendingDecision, _) {
                    final isPending = pendingDecision != null;
                    return Column(
                      children: [
                        ExpirationRow(
                          expiresAtMs: this.expiresAtMs,
                          requestedMs: request.expiresAt,
                          enabled: !isPending,
                          bodyPadding: Space.s400,
                        ),
                        EditableInfoRow(
                          label: "Label",
                          bodyPadding: Space.s400,
                          onTap: isPending
                              ? null
                              : this.labelFocusNode.requestFocus,
                          child: TextField(
                            controller: this.labelController,
                            focusNode: this.labelFocusNode,
                            enabled: !isPending,
                            maxLines: 1,
                            maxLength: 64,
                            enableSuggestions: false,
                            autocorrect: false,
                            style: InfoRow.valueStyle,
                            decoration: const InputDecoration.collapsed(
                              hintText: "Optional",
                              hintStyle: TextStyle(color: LxColors.grey650),
                            ).copyWith(counterText: ""),
                          ),
                        ),
                      ],
                    );
                  },
                )
              else ...[
                // In requested order, unrecognized last.
                InfoRow(
                  label: "Scopes",
                  value: [
                    for (final id in request.scopes)
                      if (Scope.fromStringId(s: id) != null) id,
                    ...this.unknownScopes(),
                  ].join(", "),
                  bodyPadding: Space.s400,
                ),
                if (request.permissions case final permissions
                    when permissions.isNotEmpty)
                  InfoRow(
                    label: "Permissions",
                    value: permissions.join(", "),
                    bodyPadding: Space.s400,
                  ),
                InfoRow(
                  label: "Expires",
                  value: formatExpiration(this.expiresAtMs.value),
                  bodyPadding: Space.s400,
                ),
                if (this.labelController.text.trim().nonEmpty()
                    case final label?)
                  InfoRow(
                    label: "Label",
                    value: label,
                    bodyPadding: Space.s400,
                  ),
              ],
            ],
          ),

          // Error
          Padding(
            padding: const EdgeInsets.only(top: Space.s400),
            child: ValueListenableBuilder(
              valueListenable: this.errorMessage,
              builder: (context, errorMessage, _) =>
                  ErrorMessageSection(errorMessage, bodyPadding: Space.s400),
            ),
          ),
        ],
        bottom: Padding(
          padding: const EdgeInsets.only(top: Space.s500),
          child: ValueListenableBuilder(
            valueListenable: this.pendingDecision,
            builder: (context, pendingDecision, _) {
              final isPending = pendingDecision != null;
              return Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  AnimatedFillButton(
                    label: const Text("Approve"),
                    icon: const Icon(LxIcons.next),
                    onTap: (canApprove && !isPending) ? this.onApprove : null,
                    loading: pendingDecision is CredentialDecision_Approve,
                    style: FilledButton.styleFrom(
                      backgroundColor: LxColors.moneyGoUp,
                      foregroundColor: LxColors.grey1000,
                      iconColor: LxColors.grey1000,
                    ),
                  ),
                  const SizedBox(height: Space.s300),
                  SizedBox(
                    width: double.infinity,
                    child: LxOutlinedButton(
                      label: AnimatedSwitcher(
                        duration: const Duration(milliseconds: 150),
                        child: (pendingDecision is CredentialDecision_Reject)
                            ? const SizedBox.square(
                                dimension: Fonts.size400,
                                child: CircularProgressIndicator(
                                  strokeWidth: 2.0,
                                  color: LxColors.clearB200,
                                ),
                              )
                            : const Text("Reject"),
                      ),
                      onTap: isPending ? null : this.onReject,
                    ),
                  ),
                ],
              );
            },
          ),
        ),
      ),
    );
  }
}

/// Names the requester and where the credentials will be sent.
///
/// Per the spec's phishing guidance, the heading names only a verified
/// receiving domain, or a requester verified out of band. An unverified
/// requester shows at most its uri's scheme and host, styled apart from a
/// verified domain.
class RequesterHeader extends StatelessWidget {
  const RequesterHeader({super.key, required this.requester});

  final RequesterDisplay requester;

  /// About the height of a two-line heading plus the destination.
  static const double iconSize = Space.s825;

  @override
  Widget build(BuildContext context) {
    final (name, iconUrl, destination) = switch (this.requester) {
      RequesterDisplay_Verified(:final domain, :final branding?) => (
        branding.name,
        branding.iconUrl,
        DestinationRow.verified(domain),
      ),
      RequesterDisplay_Verified(:final domain) => (
        domain,
        null,
        DestinationRow.verified(domain),
      ),
      RequesterDisplay_Unverified(:final schemeHost?) => (
        "An unverified app",
        null,
        DestinationRow.unverified(schemeHost),
      ),
      // Mailbox delivery, which has no receiving domain or uri to show.
      RequesterDisplay_Unverified() => ("An unverified app", null, null),
    };

    return Padding(
      padding: const EdgeInsets.only(top: Space.s400),
      child: Row(
        children: [
          if (iconUrl != null) ...[
            ClipRRect(
              borderRadius: BorderRadius.circular(LxRadius.r300),
              child: Image.network(
                iconUrl,
                width: iconSize,
                height: iconSize,
                fit: BoxFit.cover,
                loadingBuilder: (context, child, progress) => (progress == null)
                    ? child
                    : const ColoredBox(
                        color: LxColors.grey1000,
                        child: SizedBox.square(
                          dimension: iconSize,
                          child: Center(
                            child: SizedBox.square(
                              dimension: Fonts.size400,
                              child: CircularProgressIndicator(
                                strokeWidth: 2.0,
                                color: LxColors.clearB200,
                              ),
                            ),
                          ),
                        ),
                      ),
                // A broken icon shouldn't block the approval.
                errorBuilder: (context, error, stackTrace) =>
                    const SizedBox.square(dimension: iconSize),
              ),
            ),
            const SizedBox(width: Space.s300),
          ],
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                HeadingText(
                  text: "$name wants to connect to your wallet",
                  padding: const EdgeInsets.only(bottom: Space.s200),
                ),
                if (destination != null) destination,
              ],
            ),
          ),
        ],
      ),
    );
  }
}

/// Where the credentials will be sent, with a check if Lexe verified it.
class DestinationRow extends StatelessWidget {
  const DestinationRow({super.key, required this.icon, required this.text});

  DestinationRow.verified(String domain, {Key? key})
    : this(
        key: key,
        icon: const Icon(
          LxIcons.completedBadge,
          size: Fonts.size300,
          color: LxColors.moneyGoUp,
        ),
        text: TextSpan(text: domain),
      );

  /// A redirect to a non-https uri, which any app can claim.
  DestinationRow.unverified(String schemeHost, {Key? key})
    : this(
        key: key,
        icon: null,
        text: TextSpan(
          children: [
            const TextSpan(text: "Sent to "),
            // The rest of the uri is chosen by the requester, so it's hidden.
            TextSpan(text: "$schemeHost\u2026", style: _monoStyle),
          ],
        ),
      );

  final Icon? icon;
  final InlineSpan text;

  @override
  Widget build(BuildContext context) => Row(
    children: [
      if (this.icon case final icon?) ...[
        icon,
        const SizedBox(width: Space.s100),
      ],
      Flexible(
        child: Text.rich(
          this.text,
          style: Fonts.fontUI.copyWith(
            color: LxColors.grey600,
            fontSize: Fonts.size300,
            height: 1.2,
          ),
        ),
      ),
    ],
  );
}

/// Warns that the requested credentials can spend funds.
class SpendWarningCard extends StatelessWidget {
  const SpendWarningCard({super.key});

  @override
  Widget build(BuildContext context) => const Padding(
    padding: EdgeInsets.only(bottom: Space.s200),
    child: Card.filled(
      color: LxColors.grey1000,
      margin: EdgeInsets.zero,
      child: Padding(
        padding: EdgeInsets.all(Space.s400),
        child: Row(
          children: [
            Icon(
              LxIcons.warning,
              size: Fonts.size400,
              color: LxColors.warningText,
            ),
            SizedBox(width: Space.s300),
            Expanded(
              child: Text.rich(
                TextSpan(
                  children: [
                    TextSpan(
                      text: "This app can spend your funds. ",
                      style: TextStyle(
                        fontVariations: [Fonts.weightSemiBold],
                        color: LxColors.foreground,
                      ),
                    ),
                    TextSpan(text: "Only approve connections you initiated."),
                  ],
                ),
                style: TextStyle(
                  fontSize: Fonts.size200,
                  color: LxColors.fgSecondary,
                  height: 1.4,
                ),
              ),
            ),
          ],
        ),
      ),
    ),
  );
}

/// One requested capability.
typedef AccessItem = ({
  IconData icon,
  Widget title,
  Widget description,
  AccessKind kind,
});

enum AccessKind {
  scope,
  permissions,

  /// A scope the user can't grant.
  error,
}

/// Monospace for raw ids, keeping the surrounding size and color.
final TextStyle _monoStyle = TextStyle(fontFamily: Fonts.fontUIMono.fontFamily);

/// An [AccessItem] row inside the access [InfoCard]. Tapping toggles the
/// description, unless [onTap] is null.
class AccessRow extends StatelessWidget {
  const AccessRow({
    super.key,
    required this.item,
    required this.expanded,
    required this.onTap,
  });

  final AccessItem item;
  final bool expanded;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) => InkWell(
    onTap: this.onTap,
    child: Padding(
      // Icon glyphs have their own insets, so trim the side padding to
      // align them optically with the card's text edges.
      padding: const EdgeInsets.fromLTRB(
        Space.s400 - 3.0,
        Space.s200,
        Space.s300,
        Space.s200,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              // Matches the title's weight and color, or red for an error.
              Icon(
                this.item.icon,
                size: Fonts.size400,
                color: (this.item.kind == AccessKind.error)
                    ? LxColors.errorText
                    : LxColors.foreground,
                weight: LxIcons.weightMedium,
                grade: LxIcons.gradeNormal,
              ),
              const SizedBox(width: Space.s200),
              Expanded(
                child: DefaultTextStyle.merge(
                  style: const TextStyle(
                    fontSize: Fonts.size300,
                    fontVariations: [Fonts.weightMedium],
                    color: LxColors.foreground,
                    height: 1.25,
                  ),
                  child: this.item.title,
                ),
              ),
              if (this.onTap != null) ...[
                const SizedBox(width: Space.s200),
                AnimatedRotation(
                  turns: this.expanded ? 0.25 : 0.0,
                  duration: const Duration(milliseconds: 150),
                  child: const Icon(
                    LxIcons.nextSecondary,
                    size: Fonts.size400,
                    color: LxColors.grey650,
                  ),
                ),
              ],
            ],
          ),
          AnimatedSize(
            duration: const Duration(milliseconds: 150),
            curve: Curves.easeOut,
            alignment: Alignment.topLeft,
            child: this.expanded
                ? Padding(
                    // Align with the title, past the icon and its gap.
                    padding: const EdgeInsets.only(
                      top: Space.s100,
                      left: Fonts.size400 + Space.s200,
                    ),
                    child: DefaultTextStyle.merge(
                      style: const TextStyle(
                        fontSize: Fonts.size200,
                        color: LxColors.grey550,
                        height: 1.3,
                      ),
                      child: this.item.description,
                    ),
                  )
                : const SizedBox(width: double.infinity),
          ),
        ],
      ),
    ),
  );
}

extension on Scope {
  IconData icon() => switch (this) {
    Scope.readInfo || Scope.readPayments || Scope.read => LxIcons.view,
    Scope.receive => LxIcons.receive,
    Scope.manageChannels => LxIcons.openCloseChannel,
    Scope.spend => LxIcons.send,
    Scope.full => LxIcons.key,
  };
}
