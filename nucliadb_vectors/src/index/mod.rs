// Copyright (C) 2021 Bosutech XXI S.L.
//
// nucliadb is offered under the AGPL v3.0 and as commercial software.
// For commercial licensing, contact us at info@nuclia.com.
//
// AGPL:
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <http://www.gnu.org/licenses/>.
//

use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::memory_system::elements::*;
use crate::memory_system::lmdb_driver::LMBDStorage;
use crate::memory_system::mmap_driver::*;

pub struct Index {
    key_storage: Storage,
    vector_storage: Storage,
    lmdb_driver: LMBDStorage,
    time_stamp: u128,
    layers_len: usize,
    removed: Vec<Node>,
    entry_point: Option<EntryPoint>,
    layers_out: Vec<GraphLayer>,
    layers_in: Vec<GraphLayer>,
}

impl Debug for Index {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut debug_data = f.debug_struct("ReadIndex");
        debug_data.finish()
    }
}

impl Index {
    pub fn reader(path: &Path) -> Index {
        let key_storage = Storage::open(&path.join(KEYS_DIR));
        let vector_storage = Storage::open(&path.join(VECTORS_DIR));
        let lmdb_driver = LMBDStorage::open(path);
        let ro_txn = lmdb_driver.ro_txn();
        let log = lmdb_driver.get_log(&ro_txn);
        let layers_in = vec![];
        let mut layers_out = vec![];
        for i in 0..log.max_layer {
            let layer_out = lmdb_driver.get_layer_out(&ro_txn, i).unwrap();
            layers_out.push(layer_out);
        }
        let removed = vec![];
        ro_txn.abort().unwrap();
        Index {
            key_storage,
            vector_storage,
            lmdb_driver,
            layers_out,
            layers_in,
            removed,
            time_stamp: log.version_number,
            layers_len: log.max_layer as usize,
            entry_point: log.entry_point,
        }
    }
    pub fn writer(path: &Path) -> Index {
        let key_storage = Storage::create(&path.join(KEYS_DIR));
        let vector_storage = Storage::create(&path.join(VECTORS_DIR));
        let lmdb_driver = LMBDStorage::create(path);
        let ro_txn = lmdb_driver.ro_txn();
        let log = lmdb_driver.get_log(&ro_txn);
        let mut layers_out = vec![];
        let mut layers_in = vec![];
        for i in 0..log.max_layer {
            let layer_out = lmdb_driver.get_layer_out(&ro_txn, i).unwrap();
            let layer_in = lmdb_driver.get_layer_in(&ro_txn, i).unwrap();
            layers_out.push(layer_out);
            layers_in.push(layer_in);
        }
        let removed = vec![];
        ro_txn.abort().unwrap();
        Index {
            key_storage,
            vector_storage,
            lmdb_driver,
            layers_out,
            layers_in,
            removed,
            time_stamp: log.version_number + 1,
            layers_len: log.max_layer as usize,
            entry_point: log.entry_point,
        }
    }
    pub fn semi_mapped_similarity(&self, x: &Vector, y: Node) -> f32 {
        semi_mapped_consine_similarity(&x.raw, y, &self.vector_storage)
    }
    pub fn has_labels(&self, node: Node, labels: &[String]) -> bool {
        let txn = self.lmdb_driver.ro_txn();
        let key = String::from_byte_rpr(self.key_storage.read(node.key).unwrap());
        let all = labels
            .iter()
            .all(|label| self.lmdb_driver.has_label(&txn, &key, label));
        txn.abort().unwrap();
        all
    }
    pub fn has_node(&self, key: &str) -> bool {
        let txn = self.lmdb_driver.ro_txn();
        let exist = self.lmdb_driver.get_node(&txn, key).is_some();
        txn.abort().unwrap();
        exist
    }
    pub fn get_node_key(&self, node: Node) -> String {
        String::from_byte_rpr(self.key_storage.read(node.key).unwrap())
    }
    pub fn get_node_vector(&self, node: Node) -> Vector {
        Vector::from_byte_rpr(self.vector_storage.read(node.vector).unwrap())
    }
    pub fn reload(&mut self) {
        let txn = self.lmdb_driver.ro_txn();
        self.vector_storage.reload();
        self.key_storage.reload();
        let log = self.lmdb_driver.get_log(&txn);
        if self.time_stamp != log.version_number {
            self.time_stamp = log.version_number;
            self.entry_point = log.entry_point;
            self.layers_len = log.max_layer as usize;
            self.layers_out = Vec::with_capacity(self.layers_len);
            for i in 0..log.max_layer {
                let layer_out = self.lmdb_driver.get_layer_out(&txn, i).unwrap();
                self.layers_out.push(layer_out);
            }
        }
        txn.abort().unwrap();
    }
    pub fn commit(&mut self) {
        let mut rw_txn = self.lmdb_driver.rw_txn();
        let log = GraphLog {
            entry_point: self.entry_point,
            max_layer: self.layers_len as u64,
            version_number: self.time_stamp,
        };
        let deleted = std::mem::take(&mut self.removed);
        self.time_stamp += 1;
        for i in 0..self.layers_len {
            let layer_out = self.layers_out[i].clone();
            let layer_in = self.layers_in[i].clone();
            self.lmdb_driver
                .insert_layer_out(&mut rw_txn, i as u64, layer_out);
            self.lmdb_driver
                .insert_layer_in(&mut rw_txn, i as u64, layer_in);
        }
        for deleted in &deleted {
            let key = self.get_node_key(*deleted);
            self.lmdb_driver.remove_vector(&mut rw_txn, &key);
        }
        self.lmdb_driver.insert_log(&mut rw_txn, log);
        self.lmdb_driver
            .marked_deleted(&mut rw_txn, self.time_stamp, deleted);
        rw_txn.commit().unwrap();
    }
    pub fn run_garbage_collection(&mut self) {
        let mut rw_txn = self.lmdb_driver.rw_txn();
        let deleted = self.lmdb_driver.clear_deleted(&mut rw_txn);
        for node in deleted {
            self.vector_storage.delete_segment(node.vector);
            self.key_storage.delete_segment(node.key);
        }
        rw_txn.commit().unwrap();
    }
    pub fn no_nodes(&self) -> usize {
        if self.layers_out.is_empty() {
            0
        } else {
            self.layers_out[0].no_nodes()
        }
    }
    pub fn get_entry_point(&self) -> Option<EntryPoint> {
        self.entry_point
    }
    pub fn add_node(&mut self, key: String, vector: Vector, layer: usize) -> Node {
        let mut txn = self.lmdb_driver.rw_txn();
        let node = Node {
            key: self.key_storage.insert(&key.as_byte_rpr()),
            vector: self.vector_storage.insert(&vector.as_byte_rpr()),
        };
        self.lmdb_driver.add_node(&mut txn, key, node);
        txn.commit().unwrap();
        self.layers_len = std::cmp::max(self.layers_len, layer + 1);
        while self.layers_out.len() < self.layers_len {
            self.layers_out.push(GraphLayer::new());
            self.layers_in.push(GraphLayer::new());
        }
        for i in 0..=layer {
            self.layers_out[i].add_node(node);
            self.layers_in[i].add_node(node);
        }
        node
    }
    pub fn get_node(&self, key: &str) -> Option<Node> {
        let txn = self.lmdb_driver.ro_txn();
        let node = self.lmdb_driver.get_node(&txn, key);
        txn.abort().unwrap();
        node
    }
    pub fn get_prefixed(&self, prefix: &str) -> Vec<String> {
        let txn = self.lmdb_driver.ro_txn();
        let result = self.lmdb_driver.get_prefixed(&txn, prefix);
        txn.abort().unwrap();
        result
    }
    pub fn connect(&mut self, layer: usize, out_edge: Edge) {
        let in_edge = Edge {
            from: out_edge.to,
            to: out_edge.from,
            dist: out_edge.dist,
        };
        self.layers_out[layer].add_edge(out_edge.from, out_edge);
        self.layers_in[layer].add_edge(in_edge.from, in_edge);
    }
    pub fn disconnect(&mut self, layer: usize, source: Node, destination: Node) {
        self.layers_out[layer].remove_edge(source, destination);
        self.layers_in[layer].remove_edge(destination, source);
    }
    pub fn add_label(&mut self, key: String, label: String) {
        let mut txn = self.lmdb_driver.rw_txn();
        self.lmdb_driver.add_label(&mut txn, key, label);
        txn.commit().unwrap();
    }
    pub fn out_edges(&self, layer: usize, node: Node) -> HashMap<Node, Edge> {
        self.layers_out[layer].get_edges(node)
    }
    pub fn in_edges(&self, layer: usize, node: Node) -> HashMap<Node, Edge> {
        self.layers_in[layer].get_edges(node)
    }
    pub fn is_node_at(&self, layer: usize, node: Node) -> bool {
        self.layers_out[layer].has_node(node)
    }
    pub fn set_entry_point(&mut self, ep: EntryPoint) {
        match self.entry_point {
            Some(crnt) if crnt.layer <= ep.layer => {
                self.entry_point = Some(ep);
            }
            None => {
                self.entry_point = Some(ep);
            }
            _ => (),
        }
    }
    pub fn erase(&mut self, x: Node) {
        let mut max_layer = 0;
        // Remove x from all layers and take max non empty layer
        for layer in 0..self.layers_len {
            self.layers_out[layer].remove_node(x);
            self.layers_in[layer].remove_node(x);
            if !self.layers_out[layer].is_empty() {
                max_layer = layer;
            }
        }

        // Entry point update
        let new_entry = self.layers_out[max_layer].some_node();
        self.entry_point = new_entry.map(|node| EntryPoint {
            node,
            layer: max_layer as u64,
        });

        self.layers_len = if self.entry_point.is_none() {
            0
        } else {
            max_layer + 1
        };
        self.layers_out.truncate(self.layers_len);
        self.layers_in.truncate(self.layers_len);
        let id = String::from_byte_rpr(self.key_storage.read(x.key).unwrap());
        let mut txn = self.lmdb_driver.rw_txn();
        self.lmdb_driver.remove_vector(&mut txn, &id);
        self.removed.push(x);
        txn.commit().unwrap();
    }
    pub fn stats(&self) -> Stats {
        Stats {
            nodes_per_out_layer: self.layers_out.iter().map(|l| l.no_nodes()).collect(),
            nodes_per_in_layer: self.layers_in.iter().map(|l| l.no_nodes()).collect(),
            nodes_in_total: self.no_nodes() as usize,
            entry_point: self.entry_point,
        }
    }
    pub fn no_layers(&self) -> usize {
        self.layers_len
    }
    pub fn node_keys(&self) -> Vec<String> {
        let mut keys = vec![];
        let layer_0 = &self.layers_out[0];
        for node in layer_0.get_nodes() {
            let raw_key = self.key_storage.read(node.key).unwrap();
            let key = String::from_byte_rpr(raw_key);
            keys.push(key);
        }
        keys
    }
}

#[derive(Debug)]
pub struct Stats {
    pub nodes_per_out_layer: Vec<usize>,
    pub nodes_per_in_layer: Vec<usize>,
    pub nodes_in_total: usize,
    pub entry_point: Option<EntryPoint>,
}

#[derive(Debug, Clone)]
pub struct LockIndex {
    index: Arc<RwLock<Index>>,
}
impl From<Index> for LockIndex {
    fn from(index: Index) -> Self {
        LockIndex {
            index: Arc::new(RwLock::new(index)),
        }
    }
}

impl LockIndex {
    pub fn has_labels(&self, node: Node, labels: &[String]) -> bool {
        self.index.read().unwrap().has_labels(node, labels)
    }
    pub fn get_node_vector(&self, node: Node) -> Vector {
        self.index.read().unwrap().get_node_vector(node)
    }
    pub fn get_node_key(&self, node: Node) -> String {
        self.index.read().unwrap().get_node_key(node)
    }
    pub fn semi_mapped_similarity(&self, i: &Vector, j: Node) -> f32 {
        self.index.read().unwrap().semi_mapped_similarity(i, j)
    }
    pub fn reload(&self) {
        self.index.write().unwrap().reload()
    }
    pub fn no_nodes(&self) -> usize {
        self.index.read().unwrap().no_nodes()
    }
    pub fn is_node_at(&self, layer: usize, node: Node) -> bool {
        self.index.read().unwrap().is_node_at(layer, node)
    }
    pub fn get_entry_point(&self) -> Option<EntryPoint> {
        self.index.read().unwrap().get_entry_point()
    }
    pub fn get_node(&self, key: &str) -> Option<Node> {
        self.index.read().unwrap().get_node(key)
    }
    pub fn get_prefixed(&self, prefix: &str) -> Vec<String> {
        self.index.read().unwrap().get_prefixed(prefix)
    }
    pub fn add_node(&self, key: String, vector: Vector, layer: usize) -> Node {
        self.index.write().unwrap().add_node(key, vector, layer)
    }
    pub fn add_label(&self, key: String, label: String) {
        self.index.write().unwrap().add_label(key, label)
    }
    pub fn connect(&self, layer: usize, edge: Edge) {
        self.index.write().unwrap().connect(layer, edge)
    }
    pub fn disconnect(&self, layer: usize, source: Node, destination: Node) {
        self.index
            .write()
            .unwrap()
            .disconnect(layer, source, destination)
    }
    pub fn out_edges(&self, layer: usize, node: Node) -> HashMap<Node, Edge> {
        self.index.read().unwrap().out_edges(layer, node)
    }
    pub fn in_edges(&self, layer: usize, node: Node) -> HashMap<Node, Edge> {
        self.index.read().unwrap().in_edges(layer, node)
    }
    pub fn has_node(&self, key: &str) -> bool {
        self.index.read().unwrap().has_node(key)
    }
    pub fn set_entry_point(&self, ep: EntryPoint) {
        self.index.write().unwrap().set_entry_point(ep)
    }
    pub fn erase(&self, node: Node) {
        self.index.write().unwrap().erase(node);
    }
    pub fn commit(&mut self) {
        self.index.write().unwrap().commit()
    }
    pub fn run_garbage_collection(&mut self) {
        self.index.write().unwrap().run_garbage_collection()
    }
    pub fn stats(&self) -> Stats {
        self.index.read().unwrap().stats()
    }
    pub fn no_layers(&self) -> usize {
        self.index.read().unwrap().no_layers()
    }
    pub fn node_keys(&self) -> Vec<String> {
        self.index.read().unwrap().node_keys()
    }
}
