#!/usr/bin/env python3
"""Verify native transfer confirmations/queues against the dedicated loopback SFTP fixture."""
import argparse
import hashlib
import json
import shutil
from pathlib import Path
import time

from qa_accept_macos import Acceptance


class TransferAcceptance(Acceptance):
    """Every path belongs to the explicitly chosen QA directory or fixture root."""

    def __init__(self, directory, fixture):
        super().__init__(directory, fixture)
        self.report=directory/'transfer-results.json'
        self.results={'checks':[], 'scope':__doc__}

    def row(self, name, additive=False):
        """Select a file via the same target-bound row action used by the interface."""
        pane=self.pane(self.state())
        self.action('file_row',session=pane['owner'],attempt=pane['attempt'],request=pane['file_request'],
                    path=pane['file_path']+'/'+name,additive=additive)

    def tasks_done(self, ids, label, limit=40):
        """Require terminal states for the exact new tasks, without counting older history."""
        deadline=time.monotonic()+limit
        while time.monotonic()<deadline:
            tasks=[t for t in self.state()['transfers'] if t['id'] in ids]
            if len(tasks)==len(ids) and all(t['state'] not in ['queued','running'] for t in tasks):
                self.check(True,label)
                return tasks
            time.sleep(.05)
        raise AssertionError(label)

    def stage(self, upload, paths):
        """Use the native confirmation instead of sending straight to the backend."""
        self.action('stage_transfer',upload=upload,local_paths=[str(p) for p in paths])
        state=self.wait(lambda s:bool(s['pending_transfers']),'Native transfer confirmation opened')
        return state['pending_transfers']

    @staticmethod
    def digest(path):
        """Compare actual transferred bytes without assuming progress means correct content."""
        value=hashlib.sha256()
        with path.open('rb') as source:
            for block in iter(lambda:source.read(65536),b''): value.update(block)
        return value.hexdigest()

    def exercise(self):
        """Cover batch files/directories, cancellation, failed retries and immutable targets."""
        profile=json.loads((self.fixture/'profile.json').read_text())['profile']
        if profile['host'] not in ['127.0.0.1','::1','localhost']:
            raise AssertionError('Use only the task-owned loopback server')
        remote=Path(json.loads((self.fixture/'fixture.json').read_text())['root']).resolve()/('transfer-acceptance-'+self.root.name)
        remote.mkdir(exist_ok=True)
        local=self.root/'uploads'
        local.mkdir(exist_ok=True)
        (local/'tree/nested').mkdir(parents=True,exist_ok=True)
        (local/'one.txt').write_text('真实上传下载 中文\n'*128)
        (local/'two.bin').write_bytes(bytes(range(256))*128)
        (local/'tree/nested/你好.txt').write_text('Nested directory fixture\n')
        self.action('profile',profile=profile)
        self.action('submit_profile',connect=True)
        self.wait(lambda s:s['modal']=='trust','New fixture host awaits fingerprint confirmation')
        self.action('trust')
        connected=self.wait(lambda s:self.pane(s)['state']=='Connected' or s['modal']=='credentials',
                            'Loopback host verified for transfers')
        if connected['modal']=='credentials':
            secret=self.root/'fixture-password.txt'
            shutil.copyfile(self.fixture/'password.txt',secret)
            secret.chmod(0o600)
            self.action('fixture_credentials',file=str(secret))
            self.action('submit_credentials')
        self.wait(lambda s:self.pane(s)['state']=='Connected','Real SSH connected for transfers')
        owner=self.pane(self.state())['owner']; attempt=self.pane(self.state())['attempt']
        self.action('tool',tool='files')
        self.action('navigate',path=str(remote))
        self.wait(lambda s:self.pane(s)['file_path']==str(remote) and not self.pane(s)['file_loading'],'Empty remote target loaded')
        prepared=self.stage(True,[local/'one.txt',local/'two.bin',local/'tree'])
        self.check(len(prepared)==3 and all(t['session']==owner and t['attempt']==attempt for t in prepared),
                   'Batch confirmation fixes all source paths, target paths and original session')
        sizes={str(path):path.stat().st_size for path in [local/'one.txt',local/'two.bin']}
        sized=self.wait(lambda s:s['modal']=='transfer' and len(s['pending_transfers'])==3
                        and all(t['total']==sizes[t['local']] for t in s['pending_transfers'] if t['local'] in sizes),
                        'Regular upload sizes reach the live confirmation')
        self.check(all(t['total'] is None for t in sized['pending_transfers'] if t['local'] not in sizes),
                   'Directory retry keeps an unknown total instead of a false file size')
        self.action('dismiss')
        self.check(not (remote/'one.txt').exists() and not self.state()['transfers'],'Cancelling confirmation transfers no bytes')
        changed_directory=remote/'other'
        changed_directory.mkdir()
        stale=self.stage(True,[local/'one.txt'])
        self.action('navigate',path=str(changed_directory))
        self.wait(lambda s:self.pane(s)['file_path']==str(changed_directory) and not self.pane(s)['file_loading'],
                  'Directory changed after staging a transfer')
        rejected=self.action('confirm_transfers')
        self.check(rejected['modal']=='transfer' and rejected['transfer_view']['phase']=='review'
                   and not any(t['id']==stale[0]['id'] for t in rejected['transfers']),
                   'Changed directory rejects stale review without starting a worker')
        self.action('cancel_modal')
        self.action('navigate',path=str(remote))
        self.wait(lambda s:self.pane(s)['file_path']==str(remote) and not self.pane(s)['file_loading'],
                  'Original directory restored after stale review')
        prepared=self.stage(True,[local/'one.txt',local/'two.bin',local/'tree'])
        started=self.action('confirm_transfers')
        self.check(started['transfer_view'] is None or started['transfer_view']['phase']=='running' or started['modal'] is None,
                   'Batch leaves review immediately on first submit')
        self.action('confirm_transfers')
        self.wait(lambda s: s['modal'] is None, 'Successful upload batch closes after all tasks settle')
        self.action('tab',index=0)
        uploaded=self.tasks_done([t['id'] for t in prepared],'All batch uploads reached terminal states')
        self.check(all(sum(t['id']==record['id'] for t in self.state()['transfers'])==1 for record in prepared),
                   'Repeated submit never creates duplicate task UUIDs')
        self.check(all(t['state']=='completed' for t in uploaded),'Files and nested directory upload completed')
        self.check(all(self.digest(local/name)==self.digest(remote/name) for name in ['one.txt','two.bin','tree/nested/你好.txt']),
                   'Uploaded file and directory bytes match their originals')
        self.check(self.state()['active_tab']==0 and all(t['session']==owner for t in uploaded),'Background transfers preserve focus and target ownership')
        self.action('tab',index=1)
        self.action('navigate',path=str(remote))
        self.wait(lambda s:'tree' in self.pane(s)['files'] and not self.pane(s)['file_loading'],'Uploaded entries appear in SFTP list')
        for index,name in enumerate(['one.txt','two.bin','tree']): self.row(name,additive=index>0)
        destination=self.root/'downloads'; destination.mkdir(exist_ok=True)
        prepared=self.stage(False,[destination])
        self.check(len(prepared)==3,'Batch download uses the selected rows')
        self.action('confirm_transfers')
        self.wait(lambda s: s['modal'] is None, 'Successful download batch closes after all tasks settle')
        downloaded=self.tasks_done([t['id'] for t in prepared],'All batch downloads reached terminal states')
        self.check(all(t['state']=='completed' for t in downloaded),'Files and nested directory download completed')
        self.check(all(self.digest(local/name)==self.digest(destination/name) for name in ['one.txt','two.bin','tree/nested/你好.txt']),
                   'Downloaded file and directory bytes match their originals')
        prepared=self.stage(True,[local/'one.txt'])
        self.action('confirm_transfers')
        self.wait(lambda s: s['modal']=='transfer' and s['transfer_view'] and s['transfer_view']['phase']=='result',
                  'Failed upload batch remains on results instead of closing')
        failed=self.tasks_done([prepared[0]['id']],'Existing target produces a terminal result')[0]
        self.check(failed['state']=='failed' and bool(failed['error']),'Overwrite without explicit confirmation is rejected with a reason')
        self.check(any(t['id']==failed['id'] and t['state']=='failed' for t in self.state()['transfers']),
                   'Failure remains in persistent transfer history while results are open')
        self.action('retry_transfer',id=failed['id'])
        retried=self.wait(lambda s:s['modal']=='transfer' and s['transfer_view'] and s['transfer_view']['phase']=='review'
                          and bool(s['pending_transfers']),'Retry reopens target review')['pending_transfers'][0]
        self.check(retried['id']!=failed['id'] and retried['bytes']==0 and retried['remote']==failed['remote'],
                   'Retry creates a new task from zero with the same endpoints')
        self.action('confirm_transfers',overwrite=True)
        self.wait(lambda s: s['modal'] is None, 'Successful explicit overwrite retry closes on completion')
        done=self.tasks_done([retried['id']],'Confirmed overwrite retry reached a terminal state')[0]
        self.check(done['state']=='completed' and self.digest(remote/'one.txt')==self.digest(local/'one.txt'),'Explicit overwrite retry succeeds')
        self.check(next(t for t in self.state()['transfers'] if t['id']==failed['id'])['state']=='failed','Retry preserves the failed historical record')
        background=local/'background.bin'
        with background.open('wb') as file: file.truncate(256*1024*1024)
        prepared_background=self.stage(True,[background])
        launched=self.action('confirm_transfers')
        if launched['modal']=='transfer' and launched['transfer_view']['phase']=='running':
            self.action('transfer_background')
            self.check(self.state()['modal'] is None,
                       'Background continue closes only the live view')
        else:
            self.results.setdefault('limitations',[]).append('Background action was not observable before fast loopback completion.')
            self.save()
        background_done=self.tasks_done([prepared_background[0]['id']],
                                        'Background upload settles in its original session')[0]
        self.check(background_done['state']=='completed' and self.digest(background)==self.digest(remote/'background.bin'),
                   'Background continuation preserves exact bytes and target')
        self.wait(lambda s:'background.bin' in self.pane(s)['files'] and not self.pane(s)['file_loading'],
                  'Background completion refreshes unchanged file request')
        large=[]
        for index in range(4):
            path=local/f'cancel-{index}.bin'
            with path.open('wb') as file: file.truncate(128*1024*1024)
            large.append(path)
        prepared=self.stage(True,large)
        started=self.action('confirm_transfers')
        ids={t['id'] for t in prepared}
        state=self.wait(lambda s:len([t for t in s['transfers'] if t['id'] in ids])==len(ids)
                        and (all(t['state'] not in ['queued','running'] for t in s['transfers'] if t['id'] in ids)
                             or (any(t['state']=='queued' for t in s['transfers'] if t['id'] in ids)
                                 and any(t['state']=='running' for t in s['transfers'] if t['id'] in ids))),
                        'Large batch exposes concurrent work or completes before observation')
        tasks=[t for t in state['transfers'] if t['id'] in ids]
        if any(t['state']=='queued' for t in tasks):
            self.check(sum(t['state']=='running' for t in tasks)<=2,'At most two transfers run concurrently')
            queued=next(t for t in tasks if t['state']=='queued')
            if state['modal']=='transfer' and state['transfer_view']['phase']=='running':
                self.action('review_stop_transfers')
                fixed=self.wait(lambda s:s['modal']=='cancel_transfers' and s['transfer_stop_targets'],
                                'Real batch stop asks for fixed unfinished targets')
                self.check(set(fixed['transfer_stop_targets']['ids'])<=ids and queued['id'] in fixed['transfer_stop_targets']['ids'],
                           'Stop confirmation includes queued UUID and only this batch')
                self.action('confirm_stop_transfers')
            else:
                for task in tasks: self.action('cancel_transfer',id=task['id'])
                self.results.setdefault('limitations',[]).append('Running dialog closed before real stop confirmation; worker cancellation still tested.')
                self.save()
            cancelled=self.tasks_done(list(ids),'Cancelled tasks settle without returning to running')
            self.check(all(t['state']=='cancelled' for t in cancelled),'Queued and active task cancellation both succeed')
            time.sleep(.5)
            self.action('snapshot')
            self.check(all(t['state']=='cancelled' for t in self.state()['transfers'] if t['id'] in ids),'Late progress cannot revive cancelled tasks')
            self.check(not Path(queued['remote']).exists(),'Queued cancellation never creates its destination')
            if self.state()['modal']=='transfer':
                self.wait(lambda s:s['transfer_view'] and s['transfer_view']['phase']=='result',
                          'Cancelled batch remains on results')
                self.action('cancel_modal')
        else:
            self.check(all(t['state']=='completed' and t['total']==128*1024*1024
                           and t['bytes']==t['total'] and Path(t['remote']).stat().st_size==t['total']
                           for t in tasks),'Fast loopback uploads retain correct total and byte counts')
            self.results['limitations']=['Queued/running overlap and cancellation were too brief for native snapshots; Rust SFTP cancellation test covers the worker.']
            self.save()
        self.wait(lambda s:s['modal'] is None, 'Completed large batch has no lingering dialog')
        self.results['completed_task_ids']=[t['id'] for t in uploaded+downloaded]+[done['id']]
        self.save()
        print(json.dumps({'checks':len(self.results['checks']),'record':str(self.report)}))


def main():
    """Require a fresh isolated app and an explicitly running loopback fixture."""
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory',type=Path,required=True)
    parser.add_argument('--fixture',type=Path,required=True)
    args=parser.parse_args()
    TransferAcceptance(args.directory,args.fixture).exercise()


if __name__=='__main__': main()
