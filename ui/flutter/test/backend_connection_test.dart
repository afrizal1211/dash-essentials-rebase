import 'package:flutter_test/flutter_test.dart';
import 'package:dash_essential_ui/backend/backend_connection.dart';

void main() {
  test('paused controls are Disabled, not Live or a fake Offline', () {
    expect(
      DashConnectionState.online.serviceLabel(enabled: false),
      'Disabled',
    );
    expect(DashConnectionState.online.serviceLabel(enabled: true), 'Live');
    expect(
      DashConnectionState.offline.serviceLabel(enabled: false),
      'Offline',
    );
    expect(
      DashConnectionState.connecting.serviceLabel(enabled: false),
      'Connecting',
    );
  });
  test('only a successful handshake is Live', () {
    expect(DashConnectionState.fromNative(1), DashConnectionState.online);
    expect(DashConnectionState.fromNative(1).badgeLabel, 'LIVE');
  });

  test('startup and a confirmed connection failure remain distinct', () {
    expect(
      DashConnectionState.fromNative(-1),
      DashConnectionState.connecting,
    );
    expect(DashConnectionState.fromNative(-1).badgeLabel, 'CONNECTING');
    expect(DashConnectionState.fromNative(0), DashConnectionState.offline);
    expect(DashConnectionState.fromNative(0).badgeLabel, 'OFFLINE');
  });
}
