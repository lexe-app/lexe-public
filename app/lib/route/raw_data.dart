import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show PlatformException;
import 'package:lexeapp/components.dart'
    show
        HeadingText,
        InfoCard,
        LxBackButton,
        ScrollableSinglePageBody,
        SubheadingText;
import 'package:lexeapp/result.dart' show Err, FfiError, Ok, Result;
import 'package:lexeapp/style.dart' show Fonts, LxColors, Space;

class RawDataPage extends StatefulWidget {
  const RawDataPage({
    super.key,
    required this.title,
    required this.subtitle,
    required this.data,
  });

  final String title;
  final String subtitle;
  final Future<Result<String, Exception>> data;

  @override
  State<RawDataPage> createState() => _RawDataPageState();
}

class _RawDataPageState extends State<RawDataPage> {
  Result<String, String>? _resultData;

  @override
  void initState() {
    super.initState();
    this._loadData();
  }

  Future<void> _loadData() async {
    final result = await this.widget.data;

    if (!this.mounted) return;

    switch (result) {
      case Ok(:final ok):
        this.setState(() {
          this._resultData = Ok(ok);
        });

      case Err(:final err):
        final String errStr;
        switch (err) {
          case PlatformException(:final code, :final message):
            errStr = "$message (code=$code)";
          case FfiError(:final message):
            errStr = message;
          default:
            errStr = err.toString();
        }
        this.setState(() {
          this._resultData = Err(errStr);
        });
    }
  }

  Widget _buildDataContent() {
    switch (this._resultData) {
      case null:
        return const Padding(
          padding: EdgeInsets.only(top: Space.s400, bottom: Space.s400),
          child: Align(
            alignment: Alignment.topCenter,
            child: SizedBox.square(
              dimension: 20.0,
              child: CircularProgressIndicator(
                strokeWidth: 2.0,
                color: LxColors.fgTertiary,
              ),
            ),
          ),
        );
      case Ok(:final ok):
        return SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          child: Padding(
            padding: const EdgeInsets.all(Space.s400),
            child: SelectionArea(child: Text(ok, style: Fonts.fontUIMono)),
          ),
        );

      case Err(:final err):
        return Padding(
          padding: const EdgeInsets.all(Space.s400),
          child: Text(
            'Error: $err',
            style: Fonts.fontUI.copyWith(
              fontSize: Fonts.size200,
              color: LxColors.foreground,
              fontVariations: [Fonts.weightNormal],
            ),
          ),
        );
    }
  }

  @override
  Widget build(BuildContext context) {
    const cardPad = Space.s300;
    const horizontalPad = Space.s600 - cardPad;

    // TODO(maurice): DIY the scroll portion, so body is scrollable
    // and InfoCard is kinda a terminal feel.
    return Scaffold(
      appBar: AppBar(
        leadingWidth: Space.appBarLeadingWidth,
        leading: const LxBackButton(isLeading: true),
      ),
      body: ScrollableSinglePageBody(
        padding: const EdgeInsets.symmetric(horizontal: horizontalPad),
        body: [
          Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: cardPad),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    HeadingText(text: this.widget.title),
                    SubheadingText(text: this.widget.subtitle),
                    const SizedBox(height: Space.s500),
                  ],
                ),
              ),
              InfoCard(
                header: const Text("Raw data"),
                children: [
                  SizedBox(
                    width: double.infinity,
                    child: this._buildDataContent(),
                  ),
                ],
              ),
            ],
          ),
        ],
      ),
    );
  }
}
