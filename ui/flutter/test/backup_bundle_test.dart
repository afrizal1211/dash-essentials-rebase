import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:dash_essential_ui/backend/dash_backend.dart';

void main() {
  group('splitImportChunks', () {
    test('the parts reassemble into the payload', () {
      final String hardware = jsonEncode(<String, dynamic>{
        'state': List<String>.generate(400, (int i) => 'key$i=$i'),
        'profiles': null,
        'conditionals': null,
        'dexopt': null,
      });
      expect(splitImportChunks(hardware).join(), hardware);
    });

    test('every chunk command stays under the daemon command cap', () {
      final String hardware = jsonEncode(<String, dynamic>{
        'state': List<String>.generate(400, (int i) => 'key$i=$i'),
        'profiles': null,
        'conditionals': null,
        'dexopt': null,
      });
      final List<String> parts = splitImportChunks(hardware);
      expect(parts.length, greaterThan(1));
      for (int index = 0; index < parts.length; index++) {
        final String command = 'SET import.chunk $index ${parts[index]}';
        expect(
          utf8.encode(command).length,
          lessThanOrEqualTo(4096),
          reason: 'chunk $index exceeded the 4096-byte command cap',
        );
      }
    });

    test('a payload longer than one chunk is split at the byte budget', () {
      final List<String> parts = splitImportChunks('a' * 9000);
      expect(parts.length, 3);
      expect(parts[0].length, 3000);
      expect(parts[1].length, 3000);
      expect(parts.join(), 'a' * 9000);
    });

    test('a chunk boundary never lands inside a character', () {
      // U+20AC encodes to three UTF-8 bytes, so 1500 of them are exactly 4500
      // bytes: two chunks of 3000 bytes, split between whole characters.
      final String payload = '€' * 1500;
      final List<String> parts = splitImportChunks(payload);
      expect(parts.length, 2);
      expect(parts[0].length, 1000);
      expect(parts.join(), payload);
    });

    test('an empty payload is one empty part', () {
      expect(splitImportChunks(''), <String>['']);
    });
  });
}
