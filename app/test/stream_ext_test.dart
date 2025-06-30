import 'dart:async' show StreamController;

import 'package:flutter_test/flutter_test.dart' show expect, test;
import 'package:lexeapp/mock_time.dart' show MockTime;
import 'package:lexeapp/stream_ext.dart' show StreamExt;

Future<void> main() async {
  test("asyncMapUnbuffered", () {
    MockTime().run((time) async {
      final StreamController<int> source = StreamController.broadcast(
        sync: true,
      );

      final outFut = source.stream.asyncMapUnbuffered((foo) async {
        await Future.delayed(const Duration(milliseconds: 500));
        return foo * 2;
      }).toList();

      source.add(9); // -> 18
      source.add(5); // <skip>
      source.add(7); // <skip>

      time.advance(const Duration(milliseconds: 1000));

      source.add(3); // -> 6
      source.add(1); // <skip>

      time.advance(const Duration(milliseconds: 1000));

      await source.close();
      expect(await outFut, [18, 6]);

      assert(time.isQuiescent());
    });
  });
}
