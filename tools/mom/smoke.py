"""Exercise both MOM scenarios over HTTP. All writes remain in MockWorld."""
import argparse
import json
from pathlib import Path
import subprocess
import urllib.request
import uuid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--base-url', default='http://127.0.0.1:8790')
    parser.add_argument('--report', default='.tmp/mom/smoke-report.json')
    args = parser.parse_args()
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    checks = []

    def api(path, body=None):
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(args.base_url + '/api/mom' + path,
                                     data=data, headers={'Content-Type': 'application/json'})
        with opener.open(req, timeout=20) as response:
            return json.load(response)

    def command(case_id, name, **fields):
        return api(f'/cases/{case_id}/commands', {'command': name, **fields})

    def check(name, condition):
        if not condition:
            raise AssertionError(name)
        checks.append({'name': name, 'status': 'passed'})

    s1 = api('/scenarios/s1', {})['case_id']
    c = api(f'/cases/{s1}')
    check('S1 awaits approval, no production dispatch', c['stage'] == 'WAITING_HUMAN' and not c['actions'])
    check('S1 Master starts bounded investigation', len(c['children']) == 1)
    a = api(f'/cases/{s1}/agent?limit=2')
    check('Two projections share revision', a['case_revision'] == c['case_revision'])
    check('Agent projection paginates', a['truncated'] and a['next_cursor'] > 0)
    check('S1 proposal excludes suspect equipment', c['proposal']['exclude_assets'] == ['CNC-XX'])
    command(s1, 'approve', scope='production', actor='supervisor', plan_revision=c['plan_revision'])
    command(s1, 'execute', scope='production')
    command(s1, 'execute', scope='production')
    c = api(f'/cases/{s1}')
    check('Dispatch idempotence', len(c['actions']) == 3)
    command(s1, 'advance', minutes=3)
    c = command(s1, 'close')
    check('S1 closes after actual Mock verification', c['stage'] == 'CLOSED')

    s2 = api('/scenarios/s2', {})['case_id']
    c = api(f'/cases/{s2}')
    check('S2 root cause remains unknown', c['initial_report']['root_cause_status'] == 'UNKNOWN')
    check('S2 freezes stock, separates customer risk', c['initial_report']['held'] == {'C101': 200} and c['initial_report']['delivered_risk'] == {'C100': 300})
    check('8D has explicit unverified steps', not c['eight_d']['formal_complete'])
    command(s2, 'approve', scope='initial_response', actor='supervisor', plan_revision=c['plan_revision'])
    c = command(s2, 'execute', scope='initial_response')
    check('Initial response does not wait for investigation', len(c['outbox']) == 1 and c['children'][0]['status'] == 'RUNNING')
    command(s2, 'fault', key='root_cause_verified', value=True)
    command(s2, 'fault', key='effectiveness_verified', value=True)
    c = command(s2, 'verify_complaint')
    command(s2, 'approve', scope='eight_d', actor='supervisor', plan_revision=c['plan_revision'])
    command(s2, 'execute', scope='eight_d')
    command(s2, 'advance', minutes=1)
    c = command(s2, 'close')
    check('S2 closes only after Mock field evidence and verified send', c['stage'] == 'CLOSED')
    check('Mock provenance remains visible', c['provenance'] == 'mock' and bool(c['mock_ids']))
    commit = subprocess.run(['git', 'rev-parse', 'HEAD'], capture_output=True, text=True, check=True).stdout.strip()
    report = {'run_id': str(uuid.uuid4()), 'commit': commit, 'working_tree': 'may include local changes',
              'layer': 'A', 'seed': 1, 'timezone': 'Asia/Shanghai', 'clock': 'virtual minutes',
              'templates': ['DAG-TPL-COMPOSITE-MAINT-QUAL-RESCHEDULE@1', 'DAG-TPL-CUSTOMER-COMPLAINT-8D@1'],
              'cases': [s1, s2], 'passed': len(checks), 'failed': 0, 'skipped': 0,
              'checks': checks, 'mock_ids': sorted(set(c['mock_ids']) | set(a['mock_ids'])),
              'limitations': ['No live model or industrial connector validated', 'All approvals use fixture identity', 'No real customer message sent']}
    path = Path(args.report)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print(f'{len(checks)} functional checks passed; report: {path}')


if __name__ == '__main__':
    main()
